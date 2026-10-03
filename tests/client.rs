//! `ArcherClient` against a local JSON-RPC stub.
//!
//! Runs offline: the stub answers the handful of RPC methods the client uses
//! from an in-memory account map, over real HTTP, so the request and response
//! shapes of the `solana-client` version in use are exercised end to end.
//!
//! Needs the `client` feature: `cargo test --features client`.
#![cfg(feature = "client")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use archer_sdk::client::ArcherClient;
use archer_sdk::onchain::{
    ArcherUnit, BaseAtomsPerLot, BaseLots, MakerBook, MarketStateHeader, QuoteAtomsPerBaseUnitPerTick,
    QuoteAtomsPerLot, QuoteLots, MAKER_BOOK_DISCRIMINATOR, MAKER_KIND_LO,
    MARKET_STATE_DISCRIMINATOR,
};
use archer_sdk::{pda, ARCHER_V1_PROGRAM_ID};
use base64::Engine;
use bytemuck::Zeroable;
use serde_json::{json, Value};
use solana_program::pubkey::Pubkey;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const SLOT: u64 = 4_242;
const BASE_DECIMALS: u8 = 9;
const QUOTE_DECIMALS: u8 = 6;

/// The stub's state: the accounts it serves and every call it received.
#[derive(Default)]
struct Stub {
    /// pubkey → (owner, data)
    accounts: HashMap<Pubkey, (Pubkey, Vec<u8>)>,
    /// (method, params) in arrival order
    calls: Vec<(String, Value)>,
}

type SharedStub = Arc<Mutex<Stub>>;

fn count(stub: &SharedStub, method: &str) -> usize {
    let stub = stub.lock().unwrap();
    stub.calls.iter().filter(|(m, _)| m == method).count()
}

fn ui_account(owner: &Pubkey, data: &[u8]) -> Value {
    json!({
        "lamports": 1_000_000u64,
        "owner": owner.to_string(),
        "data": [base64::engine::general_purpose::STANDARD.encode(data), "base64"],
        "executable": false,
        "rentEpoch": 0u64,
        "space": data.len(),
    })
}

fn respond(stub: &SharedStub, req: &Value) -> Value {
    let method = req["method"].as_str().unwrap_or_default().to_string();
    let params = req["params"].clone();
    let mut stub = stub.lock().unwrap();
    stub.calls.push((method.clone(), params.clone()));

    let result = match method.as_str() {
        "getAccountInfo" => {
            let key: Pubkey = params[0].as_str().unwrap().parse().unwrap();
            let value = stub
                .accounts
                .get(&key)
                .map(|(owner, data)| ui_account(owner, data))
                .unwrap_or(Value::Null);
            json!({ "context": { "slot": SLOT }, "value": value })
        }
        // Filters are recorded, not applied: every account the program owns
        // comes back, so the client's own decoding has to skip non-books.
        "getProgramAccounts" => {
            let program: Pubkey = params[0].as_str().unwrap().parse().unwrap();
            Value::Array(
                stub.accounts
                    .iter()
                    .filter(|(_, (owner, _))| *owner == program)
                    .map(|(key, (owner, data))| {
                        json!({ "pubkey": key.to_string(), "account": ui_account(owner, data) })
                    })
                    .collect(),
            )
        }
        "getSlot" => json!(SLOT),
        "getVersion" => json!({ "solana-core": "4.3.0", "feature-set": 0 }),
        other => {
            return json!({
                "jsonrpc": "2.0",
                "id": req["id"].clone(),
                "error": { "code": -32601, "message": format!("stub: no method {other}") },
            })
        }
    };

    json!({ "jsonrpc": "2.0", "id": req["id"].clone(), "result": result })
}

/// Serve the stub on an ephemeral local port and return its URL.
async fn serve(stub: SharedStub) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());

    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let stub = Arc::clone(&stub);
            tokio::spawn(async move {
                let mut buf: Vec<u8> = Vec::new();
                // One request per iteration; the connection is kept alive.
                loop {
                    let (body_start, body_len) = loop {
                        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&buf[..pos]).to_ascii_lowercase();
                            let len = head
                                .lines()
                                .find_map(|l| l.strip_prefix("content-length:"))
                                .map(|v| v.trim().parse::<usize>().unwrap())
                                .unwrap_or(0);
                            if buf.len() >= pos + 4 + len {
                                break (pos + 4, len);
                            }
                        }
                        let mut chunk = [0u8; 4096];
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    };

                    let req: Value =
                        serde_json::from_slice(&buf[body_start..body_start + body_len]).unwrap();
                    buf.drain(..body_start + body_len);

                    let body = respond(&stub, &req).to_string();
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    if socket.write_all(response.as_bytes()).await.is_err() {
                        return;
                    }
                }
            });
        }
    });

    url
}

/// A market plus its two mints, as the stub's starting state.
struct Fixture {
    stub: SharedStub,
    market: Pubkey,
    base_mint: Pubkey,
    quote_mint: Pubkey,
}

fn mint_account(decimals: u8) -> Vec<u8> {
    // An SPL mint is 82 bytes with `decimals` at offset 44.
    let mut data = vec![0u8; 82];
    data[44] = decimals;
    data
}

fn fixture() -> Fixture {
    let market = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_unique();

    let mut header = MarketStateHeader::zeroed();
    header.discriminator = *MARKET_STATE_DISCRIMINATOR;
    header.base_mint = base_mint;
    header.quote_mint = quote_mint;
    header.base_vault = Pubkey::new_unique();
    header.quote_vault = Pubkey::new_unique();
    header.base_atoms_per_base_lot = BaseAtomsPerLot::new(1_000_000);
    header.quote_atoms_per_quote_lot = QuoteAtomsPerLot::new(1);
    header.tick_size_in_quote_atoms_per_base_unit = QuoteAtomsPerBaseUnitPerTick::new(1_000);
    header.raw_base_units_per_base_unit = 1;
    header.base_decimals = BASE_DECIMALS;
    header.quote_decimals = QUOTE_DECIMALS;

    let mut stub = Stub::default();
    stub.accounts.insert(
        market,
        (ARCHER_V1_PROGRAM_ID, bytemuck::bytes_of(&header).to_vec()),
    );
    stub.accounts
        .insert(base_mint, (spl_token::ID, mint_account(BASE_DECIMALS)));
    stub.accounts
        .insert(quote_mint, (spl_token::ID, mint_account(QUOTE_DECIMALS)));

    Fixture {
        stub: Arc::new(Mutex::new(stub)),
        market,
        base_mint,
        quote_mint,
    }
}

/// Add a limit-order maker book for `maker` at its PDA and return the address.
fn add_maker_book(fx: &Fixture, maker: Pubkey) -> Pubkey {
    let mut book = MakerBook::zeroed();
    book.discriminator = *MAKER_BOOK_DISCRIMINATOR;
    book.maker = maker;
    book.market = fx.market;
    book.kind = MAKER_KIND_LO;
    book.status = 1;
    book.quote_free = QuoteLots::new(1_000_000);
    book.base_free = BaseLots::new(1_000_000);

    let (address, _) = pda::derive_maker_book(&fx.market, &maker);
    fx.stub.lock().unwrap().accounts.insert(
        address,
        (ARCHER_V1_PROGRAM_ID, bytemuck::bytes_of(&book).to_vec()),
    );
    address
}

#[tokio::test]
async fn market_config_is_fetched_once_then_cached() {
    let fx = fixture();
    let client = ArcherClient::new(&serve(Arc::clone(&fx.stub)).await);

    let config = client.get_market_config(&fx.market).await.unwrap();
    assert_eq!(config.market_pubkey, fx.market);
    assert_eq!(config.base_mint, fx.base_mint);
    assert_eq!(config.quote_mint, fx.quote_mint);
    assert_eq!(config.base_token_program, spl_token::ID);
    assert_eq!(config.quote_token_program, spl_token::ID);
    // Market header + base mint + quote mint.
    assert_eq!(count(&fx.stub, "getAccountInfo"), 3);

    client.get_market_config(&fx.market).await.unwrap();
    assert_eq!(count(&fx.stub, "getAccountInfo"), 3, "second read is cached");

    client.invalidate_market(&fx.market);
    client.get_market_config(&fx.market).await.unwrap();
    assert_eq!(count(&fx.stub, "getAccountInfo"), 6, "invalidation refetches");
}

#[tokio::test]
async fn missing_market_is_an_error() {
    let fx = fixture();
    let client = ArcherClient::new(&serve(Arc::clone(&fx.stub)).await);

    assert!(client.get_market_config(&Pubkey::new_unique()).await.is_err());
}

#[tokio::test]
async fn maker_book_is_decoded_and_a_missing_one_is_none() {
    let fx = fixture();
    let maker = Pubkey::new_unique();
    add_maker_book(&fx, maker);
    let client = ArcherClient::new(&serve(Arc::clone(&fx.stub)).await);

    let book = client.get_maker_book(&fx.market, &maker).await.unwrap();
    assert_eq!(book.maker, maker);
    assert_eq!(book.market, fx.market);

    let found = client
        .get_maker_book_optional(&fx.market, &maker)
        .await
        .unwrap();
    assert!(found.is_some());

    // An account that does not exist is `None`, not an RPC error.
    let absent = client
        .get_maker_book_optional(&fx.market, &Pubkey::new_unique())
        .await
        .unwrap();
    assert!(absent.is_none());
}

#[tokio::test]
async fn all_maker_books_filters_by_discriminator_and_market() {
    let fx = fixture();
    let first = add_maker_book(&fx, Pubkey::new_unique());
    let second = add_maker_book(&fx, Pubkey::new_unique());
    let client = ArcherClient::new(&serve(Arc::clone(&fx.stub)).await);

    let books = client.get_all_maker_books(&fx.market).await.unwrap();
    let mut addresses: Vec<Pubkey> = books.iter().map(|(address, _)| *address).collect();
    addresses.sort();
    let mut expected = vec![first, second];
    expected.sort();
    // The market account is program-owned too, and is skipped.
    assert_eq!(addresses, expected);

    let stub = fx.stub.lock().unwrap();
    let (_, params) = stub
        .calls
        .iter()
        .find(|(method, _)| method == "getProgramAccounts")
        .expect("getProgramAccounts was called");
    assert_eq!(params[0], ARCHER_V1_PROGRAM_ID.to_string());

    let filters = params[1]["filters"].as_array().expect("filters are sent");
    let offsets: Vec<u64> = filters
        .iter()
        .map(|f| f["memcmp"]["offset"].as_u64().unwrap())
        .collect();
    assert_eq!(offsets, vec![0, 40]);
    // Raw-byte filters go over the wire as byte arrays.
    assert_eq!(
        filters[0]["memcmp"]["bytes"],
        json!(MAKER_BOOK_DISCRIMINATOR.to_vec())
    );
    assert_eq!(
        filters[1]["memcmp"]["bytes"],
        json!(fx.market.to_bytes().to_vec())
    );
    assert_eq!(params[1]["encoding"], "base64");
}

#[tokio::test]
async fn slot_is_read_from_the_rpc() {
    let fx = fixture();
    let client = ArcherClient::new(&serve(Arc::clone(&fx.stub)).await);

    assert_eq!(client.get_slot().await.unwrap(), SLOT);
}

#[tokio::test]
async fn deposit_is_built_from_the_fetched_market_config() {
    let fx = fixture();
    let client = ArcherClient::new(&serve(Arc::clone(&fx.stub)).await);
    let maker = Pubkey::new_unique();

    let ix = client
        .build_deposit(
            maker,
            &fx.market,
            1.0,
            10.0,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
        )
        .await
        .unwrap();

    assert_eq!(ix.program_id, ARCHER_V1_PROGRAM_ID);
    assert!(ix.accounts.iter().any(|meta| meta.pubkey == fx.market));
    assert!(ix
        .accounts
        .iter()
        .any(|meta| meta.pubkey == maker && meta.is_signer));
}
