use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use log::error;
use rand::{distributions::Alphanumeric, Rng};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

struct AppState {
    db: Mutex<rusqlite::Connection>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ContractRequest {
    contract_text: String,
    recipient_wallet: String,
}

#[derive(Debug, Deserialize)]
struct ValidationRequest {
    passphrase: String,
}

async fn index() -> impl Responder {
    HttpResponse::Ok()
        .content_type(mime::TEXT_HTML_UTF_8)
        .body(include_str!("../templates/index.html"))
}

async fn generate_subaddress() -> Result<(String, u32), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let response = client
        .post("http://localhost:18088/json_rpc")
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": "0",
            "method": "create_address",
            "params": {"account_index": 0, "count": 1}
        }))
        .send()
        .await?;

    let json: serde_json::Value = response.json().await?;
    let address = json["result"]["address"].as_str().unwrap().to_string();
    let index = json["result"]["address_index"].as_u64().unwrap() as u32;
    Ok((address, index))
}

async fn create_contract(
    form: web::Form<ContractRequest>,
    data: web::Data<AppState>,
) -> impl Responder {
    let (contract_wallet, address_index) = match generate_subaddress().await {
        Ok((addr, idx)) => (addr, idx),
        Err(e) => {
            error!("Subaddress generation failed: {}", e);
            return HttpResponse::InternalServerError().body("Failed to generate contract wallet");
        }
    };

    let contract_id = uuid::Uuid::new_v4().to_string();
    let passphrase = rand::thread_rng()
        .sample_iter(Alphanumeric)
        .take(12)
        .map(char::from)
        .collect::<String>();

    {
        let db = data.db.lock().unwrap();
        db.execute(
            "INSERT INTO contracts (
                contract_id, 
                passphrase,
                recipient_wallet, 
                contract_wallet, 
                contract_text, 
                address_index
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                &contract_id,
                &passphrase,
                &form.recipient_wallet,
                &contract_wallet,
                &form.contract_text,
                address_index
            ],
        )
        .unwrap();
    }

    HttpResponse::SeeOther()
        .append_header(("Location", format!("/contract/{}", contract_id)))
        .finish()
}

async fn get_contract(
    path: web::Path<String>,
    query: web::Query<HashMap<String, String>>,
    data: web::Data<AppState>,
) -> impl Responder {
    let contract_id = path.into_inner();

    let (message, msg_type, message_display) = query.get("msg").map_or(
        (String::new(), String::new(), "none".to_string()),
        |msg| match msg.as_str() {
            "success" => (
                "Funds released successfully!".to_string(),
                "success".to_string(),
                "block".to_string(),
            ),
            "invalid_passphrase" => (
                "Invalid passphrase!".to_string(),
                "error".to_string(),
                "block".to_string(),
            ),
            "transfer_failed" => (
                "Funds transfer failed!".to_string(),
                "error".to_string(),
                "block".to_string(),
            ),
            _ => (String::new(), String::new(), "none".to_string()),
        },
    );

    let contract_data = {
        let db = data.db.lock().unwrap();
        db.query_row(
            "SELECT recipient_wallet, contract_wallet, contract_text, address_index
             FROM contracts WHERE contract_id = ?1",
            [&contract_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
    };

    match contract_data {
        Ok((recipient, address, text, index)) => {
            let client = reqwest::Client::new();

            // Enhanced wallet refresh with error handling
            let refresh_res = client
                .post("http://localhost:18088/json_rpc")
                .json(&serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": "0",
                    "method": "refresh",
                    "params": {
                        "start_height": 0  // Force full chain rescan
                    }
                }))
                .send()
                .await;

            if let Err(e) = refresh_res {
                error!("Wallet refresh failed: {}", e);
            }

            // Strict balance checking with confirmations
            let balance_response = client
                .post("http://localhost:18088/json_rpc")
                .json(&serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": "0",
                    "method": "get_balance",
                    "params": {
                        "account_index": 0,
                        "address_indices": [index as u32],
                        "strict": true  // Verify against blockchain
                    }
                }))
                .send()
                .await;

            let (balance, unlocked_balance) = match balance_response {
                Ok(resp) => {
                    let json: serde_json::Value = resp.json().await.unwrap();
                    let subaddresses = json["result"]["per_subaddress"].as_array().unwrap();
                    (
                        subaddresses[0]["balance"].as_u64().unwrap() as f64 / 1e12,
                        subaddresses[0]["unlocked_balance"].as_u64().unwrap() as f64 / 1e12,
                    )
                }
                Err(_) => (0.0, 0.0),
            };

            let html = include_str!("../templates/contract.html")
                .replace("{recipient_wallet}", &recipient)
                .replace("{contract_wallet}", &address)
                .replace("{contract_text}", &text)
                .replace("{balance}", &format!("{:.12}", balance))
                .replace("{unlocked_balance}", &format!("{:.12}", unlocked_balance))
                .replace("{contract_id}", &contract_id)
                .replace("{message}", &message)
                .replace("{msg_type}", &msg_type)
                .replace("{message_display}", &message_display);

            HttpResponse::Ok()
                .content_type(mime::TEXT_HTML_UTF_8)
                .body(html)
        }
        Err(e) => {
            error!("Contract lookup failed: {}", e);
            HttpResponse::NotFound().body("Contract not found")
        }
    }
}

async fn release_funds(
    path: web::Path<String>,
    form: web::Form<ValidationRequest>,
    data: web::Data<AppState>,
) -> impl Responder {
    let contract_id = path.into_inner();

    let db = data.db.lock().unwrap();
    match db.query_row(
        "SELECT passphrase, recipient_wallet, address_index 
         FROM contracts WHERE contract_id = ?1",
        [&contract_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        },
    ) {
        Ok((stored_passphrase, recipient, index)) => {
            if form.passphrase != stored_passphrase {
                return HttpResponse::SeeOther()
                    .append_header((
                        "Location",
                        format!("/contract/{}?msg=invalid_passphrase", contract_id),
                    ))
                    .finish();
            }

            let client = reqwest::Client::new();
            match client
                .post("http://localhost:18088/json_rpc")
                .json(&serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": "0",
                    "method": "transfer",
                    "params": {
                        "destinations": [{"amount": 0, "address": recipient}],
                        "account_index": 0,
                        "subaddr_indices": [index as u32]
                    }
                }))
                .send()
                .await
            {
                Ok(_) => HttpResponse::SeeOther()
                    .append_header(("Location", format!("/contract/{}?msg=success", contract_id)))
                    .finish(),
                Err(e) => {
                    error!("Transfer error: {}", e);
                    HttpResponse::SeeOther()
                        .append_header((
                            "Location",
                            format!("/contract/{}?msg=transfer_failed", contract_id),
                        ))
                        .finish()
                }
            }
        }
        Err(_) => HttpResponse::NotFound().body("Contract not found"),
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();

    // Initialize database schema
    {
        let conn = rusqlite::Connection::open("contracts.db").unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS contracts (
                contract_id TEXT PRIMARY KEY,
                passphrase TEXT NOT NULL,
                recipient_wallet TEXT NOT NULL,
                contract_wallet TEXT NOT NULL,
                contract_text TEXT NOT NULL,
                address_index INTEGER NOT NULL
            )",
            [],
        )
        .unwrap();
    }

    HttpServer::new(move || {
        // Create NEW connection for each worker thread
        let conn =
            rusqlite::Connection::open("contracts.db").expect("Failed to open database connection");

        App::new()
            .app_data(web::Data::new(AppState {
                db: Mutex::new(conn),
            }))
            .route("/", web::get().to(index))
            .route("/contract", web::post().to(create_contract))
            .route("/contract/{contract_id}", web::get().to(get_contract))
            .route("/release/{contract_id}", web::post().to(release_funds))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
