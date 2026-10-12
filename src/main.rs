use axum::{extract::{Path, Query, State}, http::{header, HeaderValue, StatusCode}, response::Response, routing::{get, post}, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{env, net::SocketAddr, path::PathBuf, sync::Arc};
use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use tokio::{fs, sync::Mutex};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
struct Vehicle { id: String, name: String, plate: String, color: String, icon: String }
#[derive(Clone, Serialize, Deserialize)]
struct Booking {
    id: Uuid, #[serde(rename = "vehicleId")] vehicle_id: String, vehicle: String, name: String,
    #[serde(rename = "loanTime")] loan_time: String, notes: String, status: String,
    #[serde(rename = "createdAt")] created_at: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "returnName")] return_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "returnTime")] return_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] fuel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "returnNotes")] return_notes: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
enum EntryType { #[serde(rename = "income")] Income, #[serde(rename = "expense")] Expense }

#[derive(Clone, Serialize, Deserialize, Debug)]
struct FinanceEntry {
    id: String,
    amount: f64,
    #[serde(rename = "type")] kind: EntryType,
    category: String,
    note: String,
    #[serde(rename = "createdAt")] created_at: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct LearnedKeyword {
    keyword: String,
    #[serde(rename = "type")] kind: EntryType,
}

#[derive(Clone, Serialize, Deserialize)]
struct AppState {
    vehicles: Vec<Vehicle>,
    bookings: Vec<Booking>,
    #[serde(default)] finance_entries: Vec<FinanceEntry>,
    #[serde(default)] learned_keywords: Vec<LearnedKeyword>,
}
type SharedState = Arc<Mutex<AppState>>;
const STATE_FILE: &str = "data/bookings.json";

#[derive(Deserialize)]
struct BookingInput { #[serde(rename = "vehicleId")] vehicle_id: Option<String>, name: Option<String>, #[serde(rename = "loanTime")] loan_time: Option<String>, notes: Option<String> }
#[derive(Deserialize)]
struct ReturnInput { name: Option<String>, #[serde(rename = "returnTime")] return_time: Option<String>, fuel: Option<String>, notes: Option<String> }

fn vehicles() -> Vec<Vehicle> {
    [("innova-1321","Innova","DG 1321 WP","black","🚗"),("xpander-1217","Xpander","DG 1217 WP","black","🚗"),("strada","Strada","DG 8192 WP","silver","🛻"),("terios","Terios","DG 301 HS","silver","🚙")]
        .into_iter().map(|(id,name,plate,color,icon)| Vehicle { id:id.into(), name:name.into(), plate:plate.into(), color:color.into(), icon:icon.into() }).collect()
}
fn snapshot(state: &AppState) -> Value {
    let vehicles = state.vehicles.iter().map(|vehicle| {
        let booking = state.bookings.iter().find(|b| b.vehicle_id == vehicle.id && b.status == "booked");
        json!({ "id": vehicle.id, "name": vehicle.name, "plate": vehicle.plate, "color": vehicle.color, "icon": vehicle.icon,
            "status": if booking.is_some() {"booked"} else {"available"}, "booking": booking })
    }).collect::<Vec<_>>();
    json!({ "vehicles": vehicles })
}

#[derive(Deserialize)]
struct FinanceInput {
    amount: Option<f64>,
    note: Option<String>,
    #[serde(rename = "type")] kind: Option<String>,
    category: Option<String>,
}

#[derive(Deserialize)]
struct ChatInput {
    message: Option<String>,
    #[serde(rename = "type")] kind: Option<String>,
}

#[derive(Deserialize)]
struct DailyQuery {
    date: Option<String>,
}

fn local_entry_date(entry: &FinanceEntry) -> Result<NaiveDate, ()> {
    let created_at = DateTime::parse_from_rfc3339(&entry.created_at).map_err(|_| ())?;
    let wit = FixedOffset::east_opt(9 * 60 * 60).ok_or(())?;
    Ok(created_at.with_timezone(&wit).date_naive())
}

fn detect_finance_entry(note: &str, requested_kind: Option<&str>, requested_category: Option<&str>) -> (EntryType, String) {
    if let Some(kind) = requested_kind {
        let kind = match kind.to_ascii_lowercase().as_str() {
            "income" | "pemasukan" | "masuk" => EntryType::Income,
            "expense" | "pengeluaran" | "keluar" => EntryType::Expense,
            _ => {
                let words = note.to_lowercase()
                    .split(|character: char| !character.is_alphanumeric())
                    .filter(|word| !word.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                let has_any = |keywords: &[&str]| keywords.iter().any(|keyword| words.iter().any(|word| word == keyword));
                if has_any(&["gaji", "gajian", "bonus", "jualan", "pendapatan", "pemasukan", "masuk", "terima", "diterima", "dapat", "dapet", "transfer"]) {
                    EntryType::Income
                } else {
                    EntryType::Expense
                }
            }
        };
        let fallback_category = match kind {
            EntryType::Income => "Gaji",
            EntryType::Expense => "Lainnya",
        };
        return (kind, requested_category.unwrap_or(fallback_category).to_string());
    }

    match classify_known_finance_entry(note, &[]) {
        Some((kind, category)) => (kind, requested_category.unwrap_or(&category).to_string()),
        None => (EntryType::Expense, requested_category.unwrap_or("Lainnya").to_string()),
    }
}

fn classify_known_finance_entry(note: &str, learned_keywords: &[LearnedKeyword]) -> Option<(EntryType, String)> {
    let words = note
        .to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let has_any = |keywords: &[&str]| keywords.iter().any(|keyword| words.iter().any(|word| word == keyword));

    if has_any(&["gaji", "gajian", "bonus", "pendapatan", "pemasukan", "masuk", "transfer", "penjualan", "jualan", "hasil", "dapet", "dapat", "terima", "diterima"]) {
        return Some((EntryType::Income, "Gaji".to_string()));
    }
    if has_any(&["pengeluaran", "keluar", "belanja", "beli", "bayar", "makan", "pake", "pakai", "transport", "listrik", "pulsa", "sewa", "kos", "obat", "rumah", "tagihan"]) {
        return Some((EntryType::Expense, "Belanja".to_string()));
    }

    let normalized_note = normalize_keyword(note);
    learned_keywords.iter().find_map(|learned| {
        let keyword = normalize_keyword(&learned.keyword);
        let matches = !keyword.is_empty() && format!(" {normalized_note} ").contains(&format!(" {keyword} "));
        matches.then(|| {
            let category = match learned.kind {
                EntryType::Income => "Gaji",
                EntryType::Expense => "Belanja",
            };
            (learned.kind.clone(), category.to_string())
        })
    })
}

fn normalize_keyword(value: &str) -> String {
    value.to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn requested_chat_kind(value: &str) -> Option<EntryType> {
    match value.to_ascii_lowercase().as_str() {
        "income" | "pemasukan" | "masuk" => Some(EntryType::Income),
        "expense" | "pengeluaran" | "keluar" => Some(EntryType::Expense),
        _ => None,
    }
}

fn parse_chat_amount(message: &str) -> Option<(f64, String)> {
    let words = message.split_whitespace().collect::<Vec<_>>();
    let mut amount_result = None;
    let mut kept_words = Vec::new();
    let mut skip_currency = false;
    let mut skip_scale = false;

    for (index, word) in words.iter().enumerate() {
        let cleaned = word.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '.' && ch != ',');
        let lower = cleaned.to_ascii_lowercase();
        let numeric_token = if lower.starts_with("rp") && cleaned[2..].chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
            &cleaned[2..]
        } else if lower.starts_with("idr") && cleaned[3..].chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
            &cleaned[3..]
        } else {
            cleaned
        };

        if skip_currency {
            skip_currency = false;
        }
        if lower == "rp" || lower == "idr" {
            skip_currency = true;
            continue;
        }
        if skip_scale {
            skip_scale = false;
            if matches!(lower.as_str(), "rb" | "ribu" | "ribuannya" | "jt" | "juta" | "jutaannya") {
                continue;
            }
        }

        let numeric_end = numeric_token.find(|ch: char| !ch.is_ascii_digit() && ch != '.' && ch != ',').unwrap_or(numeric_token.len());
        let number_part = &numeric_token[..numeric_end];
        if number_part.is_empty() || !number_part.chars().any(|ch| ch.is_ascii_digit()) {
            kept_words.push(*word);
            continue;
        }
        let suffix = numeric_token[numeric_end..].to_ascii_lowercase();
        let next_scale = words.get(index + 1).map(|next| next.trim_matches(|ch: char| !ch.is_ascii_alphabetic()).to_ascii_lowercase());
        let scale = if matches!(suffix.as_str(), "rb" | "ribu") || (suffix.is_empty() && next_scale.as_deref().is_some_and(|s| matches!(s, "rb" | "ribu"))) {
            if suffix.is_empty() { skip_scale = true; }
            1_000.0
        } else if matches!(suffix.as_str(), "jt" | "juta") || (suffix.is_empty() && next_scale.as_deref().is_some_and(|s| matches!(s, "jt" | "juta"))) {
            if suffix.is_empty() { skip_scale = true; }
            1_000_000.0
        } else if suffix.is_empty() {
            1.0
        } else {
            kept_words.push(*word);
            continue;
        };

        let normalized_number = if scale == 1.0 {
            number_part.chars().filter(|ch| ch.is_ascii_digit()).collect::<String>()
        } else {
            number_part.replace(',', ".")
        };
        if let Ok(base) = normalized_number.parse::<f64>() {
            let amount = base * scale;
            if amount.is_finite() && amount > 0.0 {
                amount_result = Some(amount);
                continue;
            }
        }
        kept_words.push(*word);
    }

    amount_result.map(|amount| {
        let note = kept_words.join(" ").trim_matches(|ch: char| !ch.is_alphanumeric()).trim().to_string();
        (amount, note)
    })
}

fn finance_summary(entries: &[FinanceEntry]) -> Value {
    let mut income_total = 0.0;
    let mut expense_total = 0.0;
    let mut income_by_category = serde_json::Map::new();
    let mut expense_by_category = serde_json::Map::new();

    for entry in entries {
        let value = entry.amount;
        match entry.kind {
            EntryType::Income => {
                income_total += value;
                let current = income_by_category.get(&entry.category).and_then(|v| v.as_f64()).unwrap_or(0.0);
                income_by_category.insert(entry.category.clone(), serde_json::Value::from(current + value));
            }
            EntryType::Expense => {
                expense_total += value;
                let current = expense_by_category.get(&entry.category).and_then(|v| v.as_f64()).unwrap_or(0.0);
                expense_by_category.insert(entry.category.clone(), serde_json::Value::from(current + value));
            }
        }
    }

    json!({
        "income": income_total,
        "expense": expense_total,
        "balance": income_total - expense_total,
        "byCategory": {
            "income": income_by_category,
            "expense": expense_by_category,
        }
    })
}

async fn save_state(state: &AppState) {
    if let Err(error) = fs::create_dir_all("data").await {
        eprintln!("Gagal membuat folder data: {error}");
        return;
    }
    match serde_json::to_vec_pretty(state) {
        Ok(content) => {
            if let Err(error) = fs::write(STATE_FILE, content).await {
                eprintln!("Gagal menyimpan status kendaraan: {error}");
            }
        }
        Err(error) => eprintln!("Gagal serialisasi status kendaraan: {error}"),
    }
}
async fn load_state() -> AppState {
    match fs::read(STATE_FILE).await {
        Ok(content) => match serde_json::from_slice::<AppState>(&content) {
            Ok(mut state) => {
                state.vehicles = vehicles();
                state.finance_entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                state
            }
            Err(error) => {
                eprintln!("File status kendaraan tidak valid, memulai data baru: {error}");
                AppState { vehicles: vehicles(), bookings: vec![], finance_entries: vec![], learned_keywords: vec![] }
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => AppState { vehicles: vehicles(), bookings: vec![], finance_entries: vec![], learned_keywords: vec![] },
        Err(error) => {
            eprintln!("Gagal membaca status kendaraan, memulai data baru: {error}");
            AppState { vehicles: vehicles(), bookings: vec![], finance_entries: vec![], learned_keywords: vec![] }
        }
    }
}
async fn sheets_append(sheet: &str, row: Vec<String>) {
    let Some(url) = env::var("GOOGLE_APPS_SCRIPT_URL").ok() else { return };
    let Ok(spreadsheet_id) = env::var("GOOGLE_SPREADSHEET_ID") else {
        eprintln!("Gagal menyimpan ke spreadsheet: GOOGLE_SPREADSHEET_ID belum diatur.");
        return;
    };
    let payload = json!({ "spreadsheetId": spreadsheet_id, "sheetName": sheet, "row": row }).to_string();
    let client = reqwest::Client::new();
    match client.get(url).query(&[("payload", payload)]).send().await {
        Ok(response) if response.status().is_success() => match response.json::<Value>().await {
            Ok(result) if result["ok"] == true => {},
            Ok(result) => eprintln!("Gagal menyimpan ke spreadsheet: {result}"),
            Err(error) => eprintln!("Respons spreadsheet tidak valid: {error}"),
        },
        Ok(response) => eprintln!("Gagal menyimpan ke spreadsheet: {}", response.status()),
        Err(error) => eprintln!("Koneksi spreadsheet gagal: {error}"),
    }
}
async fn state_response(State(state): State<SharedState>) -> Json<Value> {
    let app = state.lock().await;
    Json(snapshot(&app))
}
async fn create_booking(State(state): State<SharedState>, Json(input): Json<BookingInput>) -> (StatusCode, Json<Value>) {
    let mut app = state.lock().await;
    let vehicle_id = input.vehicle_id.unwrap_or_default();
    let vehicle = app.vehicles.iter().find(|v| v.id == vehicle_id).cloned();
    let Some(vehicle) = vehicle else { return (StatusCode::BAD_REQUEST, Json(json!({"error":"Kendaraan tidak ditemukan."}))); };
    let name = input.name.unwrap_or_default().trim().to_string();
    let loan_time = input.loan_time.unwrap_or_default();
    if name.is_empty() || loan_time.is_empty() { return (StatusCode::BAD_REQUEST, Json(json!({"error":"Nama dan waktu peminjaman wajib diisi."}))); }
    if app.bookings.iter().any(|b| b.vehicle_id == vehicle_id && b.status == "booked") { return (StatusCode::CONFLICT, Json(json!({"error":"Mobil sedang dipinjam."}))); }
    let booking = Booking { id: Uuid::new_v4(), vehicle_id: vehicle.id.clone(), vehicle: format!("{} {}", vehicle.name, vehicle.plate).trim().into(), name, loan_time, notes: input.notes.unwrap_or_default(), status:"booked".into(), created_at: Utc::now().to_rfc3339(), return_name:None, return_time:None, fuel:None, return_notes:None };
    app.bookings.push(booking.clone());
    let response = json!({"booking": booking, "vehicles": snapshot(&app)["vehicles"]});
    save_state(&app).await;
    drop(app);
    let row = vec![response["booking"]["createdAt"].as_str().unwrap_or_default().into(), response["booking"]["vehicle"].as_str().unwrap_or_default().into(), response["booking"]["name"].as_str().unwrap_or_default().into(), response["booking"]["loanTime"].as_str().unwrap_or_default().into(), response["booking"]["notes"].as_str().unwrap_or_default().into(), "Dipinjam".into()];
    tokio::spawn(sheets_append("peminjaman", row));
    (StatusCode::CREATED, Json(response))
}
async fn return_booking(State(state): State<SharedState>, Path(id): Path<Uuid>, Json(input): Json<ReturnInput>) -> (StatusCode, Json<Value>) {
    let mut app = state.lock().await;
    let Some(booking) = app.bookings.iter_mut().find(|b| b.id == id && b.status == "booked") else { return (StatusCode::NOT_FOUND, Json(json!({"error":"Peminjaman tidak ditemukan."}))); };
    let name = input.name.unwrap_or_default().trim().to_string();
    let return_time = input.return_time.unwrap_or_default();
    let fuel = input.fuel.unwrap_or_default().trim().to_string();
    if name.is_empty() || return_time.is_empty() || fuel.is_empty() { return (StatusCode::BAD_REQUEST, Json(json!({"error":"Nama, waktu pengembalian, dan pengisian bensin wajib diisi."}))); }
    booking.return_name=Some(name); booking.return_time=Some(return_time); booking.fuel=Some(fuel); booking.return_notes=Some(input.notes.unwrap_or_default()); booking.status="returned".into();
    let returned = booking.clone();
    let response = json!({"booking": returned, "vehicles": snapshot(&app)["vehicles"]});
    save_state(&app).await;
    drop(app);
    let b = &response["booking"];
    tokio::spawn(sheets_append("pengembalian", vec![Utc::now().to_rfc3339(),b["vehicle"].as_str().unwrap_or_default().into(),b["returnName"].as_str().unwrap_or_default().into(),b["returnTime"].as_str().unwrap_or_default().into(),b["fuel"].as_str().unwrap_or_default().into(),b["returnNotes"].as_str().unwrap_or_default().into()]));
    (StatusCode::OK, Json(response))
}

async fn finance_entries(State(state): State<SharedState>) -> Json<Vec<FinanceEntry>> {
    let app = state.lock().await;
    Json(app.finance_entries.clone())
}

async fn create_finance_entry(State(state): State<SharedState>, Json(input): Json<FinanceInput>) -> (StatusCode, Json<Value>) {
    let mut app = state.lock().await;
    let amount = input.amount.unwrap_or_default();
    let note = input.note.unwrap_or_default();
    let cleaned_note = note.trim();

    if amount <= 0.0 || cleaned_note.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "Nominal dan keterangan wajib diisi." })));
    }

    let (kind, category) = detect_finance_entry(cleaned_note, input.kind.as_deref(), input.category.as_deref());
    let entry = FinanceEntry {
        id: Uuid::new_v4().to_string(),
        amount,
        kind,
        category,
        note: cleaned_note.to_string(),
        created_at: Utc::now().to_rfc3339(),
    };

    app.finance_entries.push(entry.clone());
    let summary = finance_summary(&app.finance_entries);
    save_state(&app).await;
    (StatusCode::CREATED, Json(json!({ "entry": entry, "summary": summary })))
}

async fn reset_finance_entries(State(state): State<SharedState>) -> Json<Value> {
    let mut app = state.lock().await;
    let removed = app.finance_entries.len();
    app.finance_entries.clear();
    save_state(&app).await;
    Json(json!({
        "removed": removed,
        "summary": finance_summary(&app.finance_entries)
    }))
}

async fn finance_chat(State(state): State<SharedState>, Json(input): Json<ChatInput>) -> (StatusCode, Json<Value>) {
    let message = input.message.unwrap_or_default().trim().to_string();
    if message.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "Ketik transaksi yang ingin dicatat." })));
    }
    let requested_kind = match input.kind.as_deref() {
        Some(value) => match requested_chat_kind(value) {
            Some(kind) => Some(kind),
            None => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "Pilih pemasukan atau pengeluaran." }))),
        },
        None => None,
    };

    let Some((amount, note)) = parse_chat_amount(&message) else {
        return (StatusCode::OK, Json(json!({
            "entry": null,
            "reply": "Aku belum menemukan nominalnya. Coba tulis seperti “makan siang 50rb” atau “gaji 2,5jt”."
        })));
    };
    if note.is_empty() {
        return (StatusCode::OK, Json(json!({
            "entry": null,
            "reply": "Nominalnya sudah terbaca. Tambahkan keterangannya, misalnya “makan siang 50rb”."
        })));
    }

    let mut app = state.lock().await;
    let (kind, category, learned_keyword) = if let Some(kind) = requested_kind {
        let keyword = normalize_keyword(&note);
        if keyword.is_empty() {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": "Keterangan transaksi tidak boleh kosong." })));
        }
        if let Some(learned) = app.learned_keywords.iter_mut().find(|entry| normalize_keyword(&entry.keyword) == keyword) {
            learned.kind = kind.clone();
        } else {
            app.learned_keywords.push(LearnedKeyword { keyword: keyword.clone(), kind: kind.clone() });
        }
        let category = match kind {
            EntryType::Income => "Gaji",
            EntryType::Expense => "Belanja",
        };
        (kind, category.to_string(), Some(keyword))
    } else if let Some((kind, category)) = classify_known_finance_entry(&note, &app.learned_keywords) {
        (kind, category, None)
    } else {
        return (StatusCode::OK, Json(json!({
            "entry": null,
            "needsClassification": true,
            "reply": format!("Aku belum mengenali istilah “{note}”. Ini termasuk pemasukan atau pengeluaran?")
        })));
    };
    let entry = FinanceEntry {
        id: Uuid::new_v4().to_string(),
        amount,
        kind,
        category,
        note,
        created_at: Utc::now().to_rfc3339(),
    };
    app.finance_entries.push(entry.clone());
    let summary = finance_summary(&app.finance_entries);
    save_state(&app).await;
    let mut reply = match entry.kind {
        EntryType::Income => "Pemasukan berhasil dicatat. Saldo kamu bertambah.".to_string(),
        EntryType::Expense => "Pengeluaran berhasil dicatat. Saldo kamu berkurang.".to_string(),
    };
    if let Some(keyword) = learned_keyword {
        reply.push_str(&format!(" Istilah “{keyword}” sudah ditambahkan ke daftar deteksi."));
    }

    (StatusCode::CREATED, Json(json!({ "entry": entry, "summary": summary, "reply": reply })))
}

async fn finance_summary_route(State(state): State<SharedState>) -> Json<Value> {
    let app = state.lock().await;
    Json(finance_summary(&app.finance_entries))
}

async fn finance_daily(
    State(state): State<SharedState>,
    Query(query): Query<DailyQuery>,
) -> Result<Json<Value>, StatusCode> {
    let wit = FixedOffset::east_opt(9 * 60 * 60).ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let date = match query.date {
        Some(date) => NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|_| StatusCode::BAD_REQUEST)?,
        None => Utc::now().with_timezone(&wit).date_naive(),
    };
    let app = state.lock().await;
    let entries = app.finance_entries.iter()
        .filter(|entry| local_entry_date(entry).is_ok_and(|entry_date| entry_date == date))
        .cloned()
        .collect::<Vec<_>>();
    let mut summary = finance_summary(&entries);
    summary["date"] = json!(date.to_string());
    summary["entries"] = json!(entries);
    Ok(Json(summary))
}

async fn finance_export(State(state): State<SharedState>) -> Result<Response, StatusCode> {
    let app = state.lock().await;
    let header = "tanggal,jenis,kategori,nominal,catatan\n";
    let rows = app.finance_entries.iter().map(|item| {
        let kind = match item.kind { EntryType::Income => "pemasukan", EntryType::Expense => "pengeluaran" };
        format!("{},{},{},{},\"{}\"\n", item.created_at, kind, item.category, item.amount, item.note.replace('"', "\"\""))
    }).collect::<String>();
    let csv = format!("{header}{rows}");
    let mut response = Response::new(csv.into());
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/csv; charset=utf-8"));
    response.headers_mut().insert(header::CONTENT_DISPOSITION, HeaderValue::from_str("attachment; filename=laporan-keuangan.csv").unwrap());
    Ok(response)
}

async fn static_file(uri: axum::extract::OriginalUri) -> Result<Response, StatusCode> {
    let requested = uri.0.path().trim_start_matches('/');
    let file = if requested.is_empty() { PathBuf::from("public/index.html") } else if requested.contains("..") { return Err(StatusCode::FORBIDDEN); } else { PathBuf::from(format!("public/{requested}")) };
    let content = fs::read(file).await.map_err(|_| StatusCode::NOT_FOUND)?;
    let content_type = if requested.is_empty() || requested.ends_with(".html") {"text/html; charset=utf-8"} else if requested.ends_with(".css") {"text/css; charset=utf-8"} else if requested.ends_with(".wasm") {"application/wasm"} else {"text/javascript; charset=utf-8"};
    let mut response = Response::new(content.into());
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    Ok(response)
}
#[tokio::main]
async fn main() {
    let port = env::var("PORT").unwrap_or_else(|_| "3000".into()).parse().unwrap_or(3000);
    let state = Arc::new(Mutex::new(load_state().await));
    let app = Router::new()
        .route("/api/vehicles", get(state_response))
        .route("/api/bookings", post(create_booking))
        .route("/api/bookings/{id}/return", post(return_booking))
        .route("/api/finance/entries", get(finance_entries).post(create_finance_entry).delete(reset_finance_entries))
        .route("/api/finance/chat", post(finance_chat))
        .route("/api/finance/summary", get(finance_summary_route))
        .route("/api/finance/daily", get(finance_daily))
        .route("/api/finance/export.csv", get(finance_export))
        .fallback(static_file)
        .with_state(state);
    let addr = SocketAddr::from(([0,0,0,0], port));
    println!("Aplikasi keuangan dan fleet berjalan di http://localhost:{port}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::{classify_known_finance_entry, detect_finance_entry, local_entry_date, parse_chat_amount, EntryType, FinanceEntry, LearnedKeyword};
    use chrono::NaiveDate;

    #[test]
    fn parses_chat_amount_formats_and_keeps_description() {
        assert_eq!(parse_chat_amount("makan siang 50rb"), Some((50_000.0, "makan siang".into())));
        assert_eq!(parse_chat_amount("gaji 2,5jt"), Some((2_500_000.0, "gaji".into())));
        assert_eq!(parse_chat_amount("Rp 2.500.000 gaji"), Some((2_500_000.0, "gaji".into())));
        assert_eq!(parse_chat_amount("Rp50.000 makan"), Some((50_000.0, "makan".into())));
        assert_eq!(parse_chat_amount("makan siang"), None);
    }

    #[test]
    fn classifies_incoming_and_outgoing_money_by_whole_words() {
        assert!(matches!(detect_finance_entry("uang masuk", None, None).0, EntryType::Income));
        assert!(matches!(detect_finance_entry("masuk uang makan", None, None).0, EntryType::Income));
        assert!(matches!(detect_finance_entry("gaji bulanan", None, None).0, EntryType::Income));
        assert!(matches!(detect_finance_entry("bayar kos", None, None).0, EntryType::Expense));
        assert!(matches!(detect_finance_entry("makan siang", None, None).0, EntryType::Expense));
        assert!(matches!(detect_finance_entry("masukan data", None, None).0, EntryType::Expense));
    }

    #[test]
    fn chat_classifies_incoming_phrase_before_expense_words() {
        let (amount, note) = parse_chat_amount("masuk uang makan 750rb").unwrap();
        let (kind, category) = detect_finance_entry(&note, None, None);

        assert_eq!(amount, 750_000.0);
        assert_eq!(note, "masuk uang makan");
        assert!(matches!(kind, EntryType::Income));
        assert_eq!(category, "Gaji");
    }

    #[test]
    fn unknown_chat_terms_can_be_learned_and_matched_as_phrases() {
        let learned = vec![LearnedKeyword {
            keyword: "komisi proyek".into(),
            kind: EntryType::Income,
        }];

        assert!(classify_known_finance_entry("komisi proyek tambahan", &learned)
            .is_some_and(|(kind, _)| matches!(kind, EntryType::Income)));
        assert!(classify_known_finance_entry("komisi", &learned).is_none());
        assert!(classify_known_finance_entry("kopi proyek", &learned).is_none());
    }

    #[test]
    fn groups_finance_entries_by_wit_calendar_day() {
        let entry = FinanceEntry {
            id: "test".into(),
            amount: 50_000.0,
            kind: EntryType::Expense,
            category: "Makan".into(),
            note: "makan siang".into(),
            created_at: "2026-09-29T15:30:00+00:00".into(),
        };

        assert_eq!(local_entry_date(&entry).unwrap(), NaiveDate::from_ymd_opt(2026, 9, 30).unwrap());
    }
}
