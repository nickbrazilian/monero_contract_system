use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use bip39::{Language, Mnemonic};
use rand::RngCore;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use uuid::Uuid;

struct AppState {
    db: Mutex<Connection>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ContractRequest {
    contract_text: String,
    recipient_wallet: String,
}

#[derive(Debug, Serialize)]
struct ContractResponse {
    contract_id: String,
    passphrase: String,
    recipient_wallet: String,
    generated_subaddress: String,
    contract_text: String,
    contract_link: String,
}

async fn index() -> impl Responder {
    HttpResponse::Ok()
        .content_type(mime::TEXT_HTML_UTF_8)
        .body(include_str!("../index.html"))
}

async fn generate_subaddress() -> Result<String, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let rpc_response = client
        .post("http://localhost:18088/json_rpc")
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": "0",
            "method": "create_address",
            "params": {
                "account_index": 0,
                "label": "contract_address"
            }
        }))
        .send()
        .await?;

    let response_json: serde_json::Value = rpc_response.json().await?;
    Ok(response_json["result"]["address"]
        .as_str()
        .unwrap_or_default()
        .to_string())
}

async fn create_contract(
    form: web::Json<ContractRequest>,
    data: web::Data<AppState>,
) -> impl Responder {
    let subaddress = match generate_subaddress().await {
        Ok(addr) => addr,
        Err(e) => return HttpResponse::InternalServerError().body(format!("RPC Error: {}", e)),
    };

    let contract_id = Uuid::new_v4().to_string();
    let mut entropy = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut entropy);
    let mnemonic = Mnemonic::from_entropy_in(Language::English, &entropy)
        .expect("Failed to generate mnemonic");
    let passphrase = mnemonic.to_string();

    let db = data.db.lock().unwrap();
    match db.execute(
        "INSERT INTO contracts (
            contract_id,
            passphrase,
            recipient_wallet,
            generated_subaddress,
            contract_text
        ) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            &contract_id,
            &passphrase,
            &form.recipient_wallet,
            &subaddress,
            &form.contract_text,
        ],
    ) {
        Ok(_) => (),
        Err(e) => {
            return HttpResponse::InternalServerError().body(format!("Database error: {}", e))
        }
    };

    HttpResponse::Ok().json(ContractResponse {
        contract_id: contract_id.clone(),
        passphrase,
        recipient_wallet: form.recipient_wallet.clone(),
        generated_subaddress: subaddress,
        contract_text: form.contract_text.clone(),
        contract_link: format!("http://localhost:8080/{}", contract_id),
    })
}

async fn get_contract(path: web::Path<String>, data: web::Data<AppState>) -> impl Responder {
    let contract_id = path.into_inner();
    let db = data.db.lock().unwrap();

    let result = db.query_row(
        "SELECT contract_id, passphrase, recipient_wallet, generated_subaddress, contract_text
         FROM contracts WHERE contract_id = ?1",
        [&contract_id],
        |row| {
            Ok(ContractResponse {
                contract_id: row.get(0)?,
                passphrase: row.get(1)?,
                recipient_wallet: row.get(2)?,
                generated_subaddress: row.get(3)?,
                contract_text: row.get(4)?,
                contract_link: String::new(),
            })
        },
    );

    match result {
        Ok(contract) => {
            let template = include_str!("../contract.html")
                .replace("{contract_id}", &contract.contract_id)
                .replace("{recipient_wallet}", &contract.recipient_wallet)
                .replace("{generated_subaddress}", &contract.generated_subaddress)
                .replace("{contract_text}", &contract.contract_text)
                .replace("{passphrase}", &contract.passphrase);

            HttpResponse::Ok()
                .content_type(mime::TEXT_HTML_UTF_8)
                .body(template)
        }
        Err(_) => HttpResponse::NotFound().body("Contract not found"),
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let conn = Connection::open("contracts.db").expect("Could not open database");
    conn.execute(
        "CREATE TABLE IF NOT EXISTS contracts (
            id INTEGER PRIMARY KEY,
            contract_id TEXT NOT NULL UNIQUE,
            passphrase TEXT NOT NULL,
            recipient_wallet TEXT NOT NULL,
            generated_subaddress TEXT NOT NULL,
            contract_text TEXT NOT NULL
        )",
        [],
    )
    .expect("Failed to create table");

    let app_state = web::Data::new(AppState {
        db: Mutex::new(conn),
    });

    HttpServer::new(move || {
        App::new()
            .app_data(app_state.clone())
            .route("/", web::get().to(index))
            .route("/contract", web::post().to(create_contract))
            .route("/{contract_id}", web::get().to(get_contract))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
