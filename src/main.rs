use actix_files::Files;
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
        .content_type("text/html; charset=utf-8")
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
                address_index,
                released
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
            params![
                &contract_id,
                &passphrase,
                &form.recipient_wallet,
                &contract_wallet,
                &form.contract_text,
                address_index as i64
            ],
        )
        .unwrap();
    }

    HttpResponse::SeeOther()
        .append_header((
            "Location",
            format!("/contract/{}?passphrase={}", contract_id, passphrase),
        ))
        .finish()
}

async fn get_contract(
    path: web::Path<String>,
    query: web::Query<HashMap<String, String>>,
    data: web::Data<AppState>,
) -> impl Responder {
    let contract_id = path.into_inner();
    let query_params = query.into_inner();

    let (message, msg_type, message_display, released) = {
        let db = data.db.lock().unwrap();
        let msg_data = query_params.get("msg").map_or(
            (String::new(), String::new(), "none".to_string()),
            |msg| match msg.as_str() {
                "success" => (
                    "Funds released successfully!".into(),
                    "success".into(),
                    "block".into(),
                ),
                "invalid_passphrase" => {
                    ("Invalid passphrase!".into(), "error".into(), "block".into())
                }
                "transfer_failed" => (
                    "Funds transfer failed!".into(),
                    "error".into(),
                    "block".into(),
                ),
                "already_released" => (
                    "Funds already released!".into(),
                    "error".into(),
                    "block".into(),
                ),
                "no_funds" => ("No funds available!".into(), "error".into(), "block".into()),
                "insufficient_funds" => (
                    "Minimum amount not met (0.00002 XMR after fees)!".into(),
                    "error".into(),
                    "block".into(),
                ),
                _ => (String::new(), String::new(), "none".into()),
            },
        );

        let released: bool = db
            .query_row(
                "SELECT released FROM contracts WHERE contract_id = ?1",
                [&contract_id],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(false);

        (msg_data.0, msg_data.1, msg_data.2, released)
    };

    let passphrase_warning = query_params
        .get("passphrase")
        .map(|p| {
            format!(
                r#"<div class="passphrase-warning">
            <h3>⚠️ SECURE YOUR PASSPHRASE ⚠️</h3>
            <div class="passphrase-box">{}</div>
            <p>Write this down! You need it to release funds.</p>
            <p>It won't be shown again after you leave this page!</p>
        </div>"#,
                p
            )
        })
        .unwrap_or_default();

    let (recipient, address, text, index) = {
        let db = data.db.lock().unwrap();
        match db.query_row(
            "SELECT recipient_wallet, contract_wallet, contract_text, address_index 
             FROM contracts WHERE contract_id = ?1",
            [&contract_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)? as u32,
                ))
            },
        ) {
            Ok(data) => data,
            Err(_) => return HttpResponse::NotFound().body("Contract not found"),
        }
    };

    let (balance, unlocked_balance) = {
        let client = reqwest::Client::new();
        let _ = client
            .post("http://localhost:18088/json_rpc")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "0",
                "method": "refresh",
                "params": {"start_height": 0}
            }))
            .send()
            .await;

        match client
            .post("http://localhost:18088/json_rpc")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "0",
                "method": "get_balance",
                "params": {
                    "account_index": 0,
                    "address_indices": [index],
                    "strict": true
                }
            }))
            .send()
            .await
        {
            Ok(resp) => {
                let json: serde_json::Value = resp.json().await.unwrap();
                let subaddresses = json["result"]["per_subaddress"].as_array().unwrap();
                (
                    subaddresses[0]["balance"].as_u64().unwrap_or(0) as f64 / 1e12,
                    subaddresses[0]["unlocked_balance"].as_u64().unwrap_or(0) as f64 / 1e12,
                )
            }
            Err(_) => (0.0, 0.0),
        }
    };

    let (form_display, released_display) = if released {
        ("none", "block")
    } else {
        ("block", "none")
    };

    let html = include_str!("../templates/contract.html")
        .replace("{passphrase_warning}", &passphrase_warning)
        .replace("{contract_id}", &contract_id)
        .replace("{recipient_wallet}", &recipient)
        .replace("{contract_wallet}", &address)
        .replace("{contract_text}", &text)
        .replace("{balance}", &format!("{:.12}", balance))
        .replace("{unlocked_balance}", &format!("{:.12}", unlocked_balance))
        .replace("{message}", &message)
        .replace("{msg_type}", &msg_type)
        .replace("{message_display}", &message_display)
        .replace("{form_display}", form_display)
        .replace("{released_display}", released_display);

    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(html)
}

async fn release_funds(
    path: web::Path<String>,
    form: web::Form<ValidationRequest>,
    data: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
    let contract_id = path.into_inner();

    let (stored_passphrase, recipient, index) = {
        let db = data.db.lock().map_err(|e| {
            error!("Failed to lock database: {}", e);
            actix_web::error::ErrorInternalServerError("Database error")
        })?;

        let released: bool = db
            .query_row(
                "SELECT released FROM contracts WHERE contract_id = ?1",
                [&contract_id],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(true);

        if released {
            return Ok(HttpResponse::SeeOther()
                .append_header((
                    "Location",
                    format!("/contract/{}?msg=already_released", contract_id),
                ))
                .finish());
        }

        db.query_row(
            "SELECT passphrase, recipient_wallet, address_index FROM contracts WHERE contract_id = ?1",
            [&contract_id],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? as u32,
            )),
        ).map_err(|e| {
            error!("Database error: {}", e);
            actix_web::error::ErrorInternalServerError("Database error")
        })?
    };

    if form.passphrase != stored_passphrase {
        return Ok(HttpResponse::SeeOther()
            .append_header((
                "Location",
                format!("/contract/{}?msg=invalid_passphrase", contract_id),
            ))
            .finish());
    }

    let client = reqwest::Client::new();
    let transfer_result = {
        let _ = client
            .post("http://localhost:18088/json_rpc")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "0",
                "method": "refresh",
                "params": {"start_height": 0}
            }))
            .send()
            .await;

        let balance_resp = client
            .post("http://localhost:18088/json_rpc")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "0",
                "method": "get_balance",
                "params": {
                    "account_index": 0,
                    "address_indices": [index],
                    "strict": true
                }
            }))
            .send()
            .await
            .map_err(|e| {
                error!("Balance check failed: {}", e);
                actix_web::error::ErrorInternalServerError("Balance check failed")
            })?;

        let balance_json: serde_json::Value = balance_resp.json().await.map_err(|e| {
            error!("JSON parsing failed: {}", e);
            actix_web::error::ErrorInternalServerError("JSON parsing failed")
        })?;

        let unlocked_balance = balance_json["result"]["unlocked_balance"]
            .as_u64()
            .unwrap_or(0);

        if unlocked_balance < 20_000 {
            return Ok(HttpResponse::SeeOther()
                .append_header((
                    "Location",
                    format!("/contract/{}?msg=insufficient_funds", contract_id),
                ))
                .finish());
        }

        client
            .post("http://localhost:18088/json_rpc")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "0",
                "method": "sweep_all",
                "params": {
                    "address": recipient,
                    "account_index": 0,
                    "subaddr_indices": [index],
                    "priority": 1,
                    "do_not_relay": false,
                    "get_tx_keys": true
                }
            }))
            .send()
            .await
    };

    let db = data.db.lock().map_err(|e| {
        error!("Failed to lock database: {}", e);
        actix_web::error::ErrorInternalServerError("Database error")
    })?;

    match transfer_result {
        Ok(res) => {
            let json: serde_json::Value = res.json().await.unwrap();
            if json.get("result").is_some() {
                db.execute(
                    "UPDATE contracts SET released = TRUE WHERE contract_id = ?1",
                    [&contract_id],
                )
                .map_err(|e| {
                    error!("Database update failed: {}", e);
                    actix_web::error::ErrorInternalServerError("Database update failed")
                })?;

                Ok(HttpResponse::SeeOther()
                    .append_header(("Location", format!("/contract/{}?msg=success", contract_id)))
                    .finish())
            } else {
                error!("Transfer failed: {}", json["error"]);
                Ok(HttpResponse::SeeOther()
                    .append_header((
                        "Location",
                        format!("/contract/{}?msg=transfer_failed", contract_id),
                    ))
                    .finish())
            }
        }
        Err(e) => {
            error!("Transfer error: {}", e);
            Ok(HttpResponse::SeeOther()
                .append_header((
                    "Location",
                    format!("/contract/{}?msg=transfer_error", contract_id),
                ))
                .finish())
        }
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();

    let conn = rusqlite::Connection::open("contracts.db").unwrap();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS contracts (
            contract_id TEXT PRIMARY KEY,
            passphrase TEXT NOT NULL,
            recipient_wallet TEXT NOT NULL,
            contract_wallet TEXT NOT NULL,
            contract_text TEXT NOT NULL,
            address_index INTEGER NOT NULL,
            released INTEGER NOT NULL DEFAULT 0
        )",
    )
    .unwrap();

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(AppState {
                db: Mutex::new(rusqlite::Connection::open("contracts.db").unwrap()),
            }))
            .service(web::resource("/").route(web::get().to(index)))
            .service(web::resource("/contract").route(web::post().to(create_contract)))
            .service(web::resource("/contract/{contract_id}").route(web::get().to(get_contract)))
            .service(web::resource("/release/{contract_id}").route(web::post().to(release_funds)))
            // Serve static files (e.g. CSS)
            .service(Files::new("/static", "./static").show_files_listing())
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
