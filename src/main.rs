use axum::{extract::{Extension, Path, Query, Request, State}, http::{header, HeaderValue, StatusCode}, middleware::{self, Next}, response::{IntoResponse, Response}, routing::{get, post}, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::{HashMap, HashSet}, env, net::SocketAddr, path::PathBuf, sync::{Arc, OnceLock}, time::{Duration, Instant}};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, SecondsFormat, Utc};
use tokio::{fs, sync::Mutex};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
struct Vehicle { id: String, name: String, plate: String, color: String, icon: String }
#[derive(Clone, Serialize, Deserialize)]
struct Booking {
    id: Uuid, #[serde(rename = "vehicleId")] vehicle_id: String, vehicle: String, name: String,
    #[serde(rename = "nomorST", default)] nomor_st: String,
    #[serde(rename = "loanTime")] loan_time: String, notes: String, status: String,
    #[serde(rename = "loanType", default)] loan_type: Option<String>,
    #[serde(rename = "createdAt")] created_at: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "checkInTime", default)] check_in_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "checkOutTime", default)] check_out_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "returnName")] return_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "returnTime")] return_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] fuel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "returnNotes")] return_notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "returnValidationError", default)] return_validation_error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
struct AuthSession { role: String, username: String }
#[derive(Deserialize)]
struct LoginInput { username: String, password: String }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    spreadsheet_booking_counts: Option<HashMap<String, usize>>,
}
type SharedState = Arc<Mutex<AppState>>;
const STATE_FILE: &str = "data/bookings.json";
static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
static EMPLOYEE_NAMES_CACHE: OnceLock<Mutex<Option<(Instant, Vec<String>)>>> = OnceLock::new();
static SPREADSHEET_COUNTS_CACHE: OnceLock<Mutex<Option<(Instant, HashMap<String, usize>)>>> = OnceLock::new();
static AUTH_SESSIONS: OnceLock<Mutex<HashMap<String, (AuthSession, Instant)>>> = OnceLock::new();
static RETURN_VALIDATIONS_IN_PROGRESS: OnceLock<Mutex<HashSet<Uuid>>> = OnceLock::new();
static SHEETS_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(8))
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .expect("HTTP client configuration must be valid")
    })
}
#[derive(Deserialize)]
struct BookingInput {
    #[serde(rename = "vehicleId")] vehicle_id: Option<String>,
    #[serde(rename = "nomorST")] nomor_st: Option<String>,
    name: Option<String>,
    #[serde(rename = "loanTime")] loan_time: Option<String>,
    #[serde(rename = "loanType")] loan_type: Option<String>,
    notes: Option<String>,
}
#[derive(Deserialize)]
struct ReturnInput { name: Option<String>, #[serde(rename = "returnTime")] return_time: Option<String>, fuel: Option<String>, notes: Option<String> }
#[derive(Deserialize)]
struct EmptyInput {}

fn vehicles() -> Vec<Vehicle> {
    [("innova-1321","Innova","DG 1321 WP","black","🚗"),("xpander-1217","Xpander","DG 1217 WP","black","🚗"),("strada","Strada","DG 8192 WP","silver","🛻"),("terios","Terios","DG 301 HS","silver","🚙")]
        .into_iter().map(|(id,name,plate,color,icon)| Vehicle { id:id.into(), name:name.into(), plate:plate.into(), color:color.into(), icon:icon.into() }).collect()
}
fn snapshot(state: &AppState) -> Value {
    let vehicles = state.vehicles.iter().map(|vehicle| {
        let booking = state.bookings.iter().find(|b| b.vehicle_id == vehicle.id && matches!(b.status.as_str(), "booked" | "pending" | "return_pending"));
        let booking_count = state.bookings.iter().filter(|b| b.vehicle_id == vehicle.id).count();
        let booking = booking.map(|booking| {
            let mut value = serde_json::to_value(booking).expect("booking serialization must succeed");
            let (elapsed_minutes, over_limit) = booking_timing(booking, Utc::now().naive_utc());
            value["elapsedMinutes"] = elapsed_minutes.map_or(Value::Null, |minutes| json!(minutes));
            value["overLimit"] = json!(over_limit);
            value
        });
        json!({ "id": vehicle.id, "name": vehicle.name, "plate": vehicle.plate, "color": vehicle.color, "icon": vehicle.icon,
            "status": if booking.is_some() {"booked"} else {"available"}, "booking": booking, "bookingCount": booking_count })
    }).collect::<Vec<_>>();
    json!({ "vehicles": vehicles })
}
fn duration_limit_minutes(loan_type: Option<&str>) -> Option<i64> {
    match loan_type {
        Some("full_day") => Some(570),
        Some("half_day") => Some(300),
        _ => None,
    }
}
fn parse_booking_time(value: &str) -> Option<NaiveDateTime> {
    DateTime::parse_from_rfc3339(value).map(|time| time.naive_utc()).ok()
        .or_else(|| NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S").ok())
        .or_else(|| NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M").ok())
}
fn booking_timing(booking: &Booking, now: NaiveDateTime) -> (Option<i64>, bool) {
    let Some(start) = booking.check_in_time.as_deref().and_then(parse_booking_time) else {
        return (None, false);
    };
    let end = booking.check_out_time.as_deref().and_then(parse_booking_time).unwrap_or(now);
    let elapsed = (end - start).num_minutes().max(0);
    let over_limit = duration_limit_minutes(booking.loan_type.as_deref())
        .is_some_and(|limit| elapsed > limit);
    (Some(elapsed), over_limit)
}
fn normalize_vehicle_label(value: &str) -> String {
    value.chars().filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase()).collect()
}
fn spreadsheet_booking_count(vehicle: &Vehicle, counts: &HashMap<String, usize>) -> usize {
    let name = normalize_vehicle_label(&vehicle.name);
    let plate = normalize_vehicle_label(&vehicle.plate);
    counts.iter().filter(|(label, _)| {
        let label = normalize_vehicle_label(label);
        label == name || label.starts_with(&name) || (!plate.is_empty() && label.contains(&plate))
    }).map(|(_, count)| count).sum()
}
fn snapshot_with_spreadsheet_counts(state: &AppState, counts: &HashMap<String, usize>) -> Value {
    let mut result = snapshot(state);
    if let Some(vehicle_values) = result["vehicles"].as_array_mut() {
        for (vehicle_value, vehicle) in vehicle_values.iter_mut().zip(&state.vehicles) {
            let spreadsheet_count = spreadsheet_booking_count(vehicle, counts);
            let local_count = state.bookings.iter().filter(|booking| booking.vehicle_id == vehicle.id).count();
            vehicle_value["bookingCount"] = json!(spreadsheet_count.max(local_count));
        }
    }
    result
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
                AppState { vehicles: vehicles(), bookings: vec![], finance_entries: vec![], learned_keywords: vec![], spreadsheet_booking_counts: None }
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => AppState { vehicles: vehicles(), bookings: vec![], finance_entries: vec![], learned_keywords: vec![], spreadsheet_booking_counts: None },
        Err(error) => {
            eprintln!("Gagal membaca status kendaraan, memulai data baru: {error}");
            AppState { vehicles: vehicles(), bookings: vec![], finance_entries: vec![], learned_keywords: vec![], spreadsheet_booking_counts: None }
        }
    }
}
async fn sheets_request(payload: Value) -> Result<Value, String> {
    let url = env::var("GOOGLE_APPS_SCRIPT_URL")
        .map_err(|_| "GOOGLE_APPS_SCRIPT_URL belum diatur".to_string())?;
    if url.trim().is_empty() {
        return Err("GOOGLE_APPS_SCRIPT_URL kosong".into());
    }
    let payload = payload.to_string();
    let response = http_client().get(url).query(&[("payload", payload)]).send().await
        .map_err(|error| format!("Koneksi spreadsheet gagal: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("Google Apps Script merespons dengan status {}", response.status()));
    }
    let result = response.json::<Value>().await
        .map_err(|error| format!("Respons spreadsheet tidak valid: {error}"))?;
    if result["ok"] != true {
        return Err(format!(
            "Google Apps Script menolak permintaan: {}",
            result["error"].as_str().unwrap_or("kesalahan tidak diketahui")
        ));
    }
    Ok(result)
}
async fn sheets_append(sheet: &str, row: Vec<String>) -> Result<(), String> {
    let Ok(url) = env::var("GOOGLE_APPS_SCRIPT_URL") else {
        eprintln!("Google Spreadsheet tidak dikonfigurasi; data {sheet} hanya tersimpan secara lokal.");
        return Ok(());
    };
    if url.trim().is_empty() {
        eprintln!("Google Spreadsheet tidak dikonfigurasi; data {sheet} hanya tersimpan secara lokal.");
        return Ok(());
    }
    let spreadsheet_id = env::var("GOOGLE_SPREADSHEET_ID")
        .map_err(|_| "GOOGLE_SPREADSHEET_ID belum diatur".to_string())?;
    let _write_guard = SHEETS_WRITE_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    sheets_request(json!({ "spreadsheetId": spreadsheet_id, "sheetName": sheet, "row": row })).await?;
    Ok(())
}
async fn sheets_reset_bookings() -> Result<Value, String> {
    let Ok(url) = env::var("GOOGLE_APPS_SCRIPT_URL") else {
        return Ok(json!({"removed": {}}));
    };
    if url.trim().is_empty() {
        return Ok(json!({"removed": {}}));
    }
    let spreadsheet_id = env::var("GOOGLE_SPREADSHEET_ID")
        .map_err(|_| "GOOGLE_SPREADSHEET_ID belum diatur".to_string())?;
    let _write_guard = SHEETS_WRITE_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    sheets_request(json!({
        "action": "resetBookings",
        "spreadsheetId": spreadsheet_id
    })).await
}
async fn sheets_append_booking(booking: &Booking) -> Result<(), String> {
    let loan_type = match booking.loan_type.as_deref() {
        Some("full_day") => "Full Day (9,5 jam)",
        Some("half_day") => "Half Day (5 jam)",
        _ => "",
    };
    let row10 = vec![
        booking.created_at.clone(), booking.vehicle.clone(), booking.name.clone(),
        booking.loan_time.clone(), booking.nomor_st.clone(), booking.notes.clone(),
        booking.check_in_time.clone().unwrap_or_default(),
        booking.check_out_time.clone().unwrap_or_default(),
        "Dipinjam".into(), loan_type.into(),
    ];
    let row8 = vec![
        booking.created_at.clone(), booking.vehicle.clone(), booking.name.clone(),
        booking.loan_time.clone(), booking.nomor_st.clone(), booking.notes.clone(),
        "Dipinjam".into(), loan_type.into(),
    ];
    let notes_with_type = if booking.notes.is_empty() {
        format!("Tipe peminjaman: {loan_type}")
    } else {
        format!("{} | Tipe peminjaman: {loan_type}", booking.notes)
    };
    let row7 = vec![
        booking.created_at.clone(), booking.vehicle.clone(), booking.name.clone(),
        booking.loan_time.clone(), booking.nomor_st.clone(), notes_with_type.clone(),
        "Dipinjam".into(),
    ];
    let mut notes_with_details = Vec::new();
    if !booking.nomor_st.is_empty() {
        notes_with_details.push(format!("Nomor ST: {}", booking.nomor_st));
    }
    if !booking.notes.is_empty() {
        notes_with_details.push(booking.notes.clone());
    }
    notes_with_details.push(format!("Tipe peminjaman: {loan_type}"));
    let row6 = vec![
        booking.created_at.clone(), booking.vehicle.clone(), booking.name.clone(),
        booking.loan_time.clone(), notes_with_details.join(" | "), "Dipinjam".into(),
    ];
    let mut last_error = None;
    for row in [row10, row8, row7, row6] {
        match sheets_append("peminjaman", row).await {
            Ok(()) => return Ok(()),
            Err(error) if error.contains("Jumlah kolom tidak sesuai") => last_error = Some(error),
            Err(error) => return Err(error),
        }
    }
    Err(last_error.unwrap_or_else(|| "Format kolom spreadsheet tidak didukung".into()))
}
async fn sheets_booking_counts() -> Result<Option<HashMap<String, usize>>, String> {
    let Ok(url) = env::var("GOOGLE_APPS_SCRIPT_URL") else {
        return Ok(None);
    };
    if url.trim().is_empty() {
        return Ok(None);
    }
    let spreadsheet_id = env::var("GOOGLE_SPREADSHEET_ID")
        .map_err(|_| "GOOGLE_SPREADSHEET_ID belum diatur".to_string())?;
    let result = sheets_request(json!({
        "action": "bookingCounts",
        "spreadsheetId": spreadsheet_id,
        "sheetName": "peminjaman"
    })).await?;
    let counts = serde_json::from_value::<HashMap<String, usize>>(result["counts"].clone())
        .map_err(|error| format!("Data frekuensi spreadsheet tidak valid: {error}"))?;
    Ok(Some(counts))
}
async fn cached_sheets_booking_counts() -> Result<Option<HashMap<String, usize>>, String> {
    let cache = SPREADSHEET_COUNTS_CACHE.get_or_init(|| Mutex::new(None));
    let mut cached_counts = cache.lock().await;
    if let Some((cached_at, counts)) = cached_counts.as_ref() {
        if cached_at.elapsed() < Duration::from_secs(60) {
            return Ok(Some(counts.clone()));
        }
    }
    let counts = sheets_booking_counts().await?;
    if let Some(counts) = &counts {
        *cached_counts = Some((Instant::now(), counts.clone()));
    }
    Ok(counts)
}
fn peek_sheets_booking_counts() -> Option<(HashMap<String, usize>, bool)> {
    let cached_counts = SPREADSHEET_COUNTS_CACHE.get_or_init(|| Mutex::new(None)).try_lock().ok()?;
    cached_counts.as_ref().map(|(cached_at, counts)| {
        (counts.clone(), cached_at.elapsed() >= Duration::from_secs(60))
    })
}
async fn sheets_employee_names() -> Result<Vec<String>, String> {
    let spreadsheet_id = env::var("GOOGLE_SPREADSHEET_ID")
        .map_err(|_| "GOOGLE_SPREADSHEET_ID belum diatur".to_string())?;
    let result = sheets_request(json!({
        "action": "employeeNames",
        "spreadsheetId": spreadsheet_id
    })).await?;
    serde_json::from_value::<Vec<String>>(result["employees"].clone())
        .map_err(|error| format!("Daftar nama pegawai dari spreadsheet tidak valid: {error}"))
}
async fn cached_sheets_employee_names() -> Result<Vec<String>, String> {
    let cache = EMPLOYEE_NAMES_CACHE.get_or_init(|| Mutex::new(None));
    let mut cached_names = cache.lock().await;
    if let Some((cached_at, names)) = cached_names.as_ref() {
        if cached_at.elapsed() < Duration::from_secs(900) {
            return Ok(names.clone());
        }
    }
    let names = sheets_employee_names().await?;
    *cached_names = Some((Instant::now(), names.clone()));
    Ok(names)
}
async fn sheets_update_booking_check_time(booking: &Booking, check_in: bool) -> Result<(), String> {
    let Ok(url) = env::var("GOOGLE_APPS_SCRIPT_URL") else {
        return Ok(());
    };
    if url.trim().is_empty() {
        return Ok(());
    }
    let spreadsheet_id = env::var("GOOGLE_SPREADSHEET_ID")
        .map_err(|_| "GOOGLE_SPREADSHEET_ID belum diatur".to_string())?;
    let (key, value) = if check_in {
        ("checkInTime", booking.check_in_time.as_deref())
    } else {
        ("checkOutTime", booking.check_out_time.as_deref())
    };
    let value = value.ok_or_else(|| "Waktu check-in/check-out tidak tersedia".to_string())?;
    let _write_guard = SHEETS_WRITE_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let mut payload = json!({
        "action": "updateBookingCheckTimes",
        "spreadsheetId": spreadsheet_id,
        "sheetName": "peminjaman",
        "bookingCreatedAt": booking.created_at
    });
    payload[key] = json!(value);
    sheets_request(payload).await.map(|_| ())
}
async fn state_response(State(state): State<SharedState>) -> Json<Value> {
    let cached_counts = peek_sheets_booking_counts();
    if cached_counts.as_ref().is_none_or(|(_, stale)| *stale)
        && env::var("GOOGLE_APPS_SCRIPT_URL").is_ok_and(|url| !url.trim().is_empty())
    {
        tokio::spawn(async {
            if let Err(error) = cached_sheets_booking_counts().await {
                eprintln!("Gagal memperbarui cache frekuensi spreadsheet: {error}");
            }
        });
    }
    let mut app = state.lock().await;
    let mut response = match cached_counts {
        Some((counts, _)) => {
            app.spreadsheet_booking_counts = Some(counts.clone());
            snapshot_with_spreadsheet_counts(&app, &counts)
        }
        None => app.spreadsheet_booking_counts.as_ref()
            .map(|counts| snapshot_with_spreadsheet_counts(&app, counts))
            .unwrap_or_else(|| snapshot(&app)),
    };
    if response["vehicles"].is_null() {
        response["vehicles"] = json!([]);
    }
    Json(response)
}
async fn employee_names() -> Result<Json<Vec<String>>, (StatusCode, Json<Value>)> {
    cached_sheets_employee_names().await.map(Json).map_err(|error| {
        eprintln!("Gagal memuat nama pegawai dari Google Spreadsheet: {error}");
        (StatusCode::BAD_GATEWAY, Json(json!({"error": format!("Daftar nama pegawai gagal dimuat: {error}")})))
    })
}

async fn login(Json(input): Json<LoginInput>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let configured_accounts = [
        ("admin", env::var("FLEET_ADMIN_USERNAME"), env::var("FLEET_ADMIN_PASSWORD")),
        ("user", env::var("FLEET_USER_USERNAME"), env::var("FLEET_USER_PASSWORD")),
    ];
    let mut role = None;
    for (account_role, username, password) in configured_accounts {
        let (Ok(username), Ok(password)) = (username, password) else {
            return Err((StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"Akun login belum dikonfigurasi di server."}))));
        };
        if input.username == username && input.password == password {
            role = Some(account_role);
        }
    }
    let Some(role) = role else {
        return Err((StatusCode::UNAUTHORIZED, Json(json!({"error":"Username atau password salah."}))));
    };
    let session = AuthSession { role: role.into(), username: input.username };
    let token = Uuid::new_v4().to_string();
    let sessions = AUTH_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()));
    sessions.lock().await.insert(token.clone(), (session.clone(), Instant::now()));
    Ok(Json(json!({"token": token, "role": session.role, "username": session.username})))
}

async fn authenticate(mut request: Request, next: Next) -> Response {
    let token = request.headers().get(header::AUTHORIZATION)
        .and_then(|header| header.to_str().ok())
        .and_then(|header| header.strip_prefix("Bearer "));
    let Some(token) = token else {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error":"Silakan login terlebih dahulu."}))).into_response();
    };
    let sessions = AUTH_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut sessions = sessions.lock().await;
    sessions.retain(|_, (_, last_activity)| last_activity.elapsed() < Duration::from_secs(30 * 60));
    let Some(session) = sessions.get_mut(token).map(|(session, last_activity)| {
        *last_activity = Instant::now();
        session.clone()
    }) else {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error":"Sesi login berakhir. Silakan login kembali."}))).into_response();
    };
    request.extensions_mut().insert(session);
    drop(sessions);
    next.run(request).await
}

async fn no_cache_api(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, no-cache, must-revalidate, max-age=0"),
    );
    response.headers_mut().insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    response
}

async fn current_session(Extension(session): Extension<AuthSession>) -> Json<AuthSession> {
    Json(session)
}

async fn logout(Extension(session): Extension<AuthSession>, request: Request) -> StatusCode {
    if let Some(token) = request.headers().get(header::AUTHORIZATION)
        .and_then(|header| header.to_str().ok())
        .and_then(|header| header.strip_prefix("Bearer "))
    {
        AUTH_SESSIONS.get_or_init(|| Mutex::new(HashMap::new())).lock().await.remove(token);
    }
    let _ = session;
    StatusCode::NO_CONTENT
}

async fn pending_returns(State(state): State<SharedState>, Extension(session): Extension<AuthSession>) -> Result<Json<Vec<Booking>>, (StatusCode, Json<Value>)> {
    if session.role != "admin" {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error":"Akses khusus admin."}))));
    }
    let app = state.lock().await;
    Ok(Json(app.bookings.iter().filter(|booking| booking.status == "return_pending").cloned().collect()))
}

async fn reset_bookings(State(state): State<SharedState>, Extension(session): Extension<AuthSession>) -> (StatusCode, Json<Value>) {
    if session.role != "admin" {
        return (StatusCode::FORBIDDEN, Json(json!({"error":"Akses khusus admin."})));
    }
    let mut app = state.lock().await;
    let spreadsheet_result = match sheets_reset_bookings().await {
        Ok(result) => result,
        Err(error) => {
            eprintln!("Gagal mereset data peminjaman dan pengembalian: {error}");
            return (StatusCode::BAD_GATEWAY, Json(json!({"error":format!("Data spreadsheet gagal direset: {error}")})));
        }
    };
    let removed = app.bookings.len();
    app.bookings.clear();
    app.spreadsheet_booking_counts = Some(HashMap::new());
    save_state(&app).await;
    *SPREADSHEET_COUNTS_CACHE.get_or_init(|| Mutex::new(None)).lock().await =
        Some((Instant::now(), HashMap::new()));
    (StatusCode::OK, Json(json!({
        "removed": removed,
        "spreadsheet": spreadsheet_result["removed"]
    })))
}

async fn validate_return(State(state): State<SharedState>, Path(id): Path<Uuid>, Extension(session): Extension<AuthSession>) -> (StatusCode, Json<Value>) {
    if session.role != "admin" {
        return (StatusCode::FORBIDDEN, Json(json!({"error":"Akses khusus admin."})));
    }
    let submitted = {
        let app = state.lock().await;
        let Some(booking) = app.bookings.iter().find(|booking| booking.id == id && booking.status == "return_pending").cloned() else {
            return (StatusCode::NOT_FOUND, Json(json!({"error":"Pengajuan pengembalian tidak ditemukan."})));
        };
        booking
    };
    let in_progress = RETURN_VALIDATIONS_IN_PROGRESS.get_or_init(|| Mutex::new(HashSet::new()));
    {
        let mut in_progress = in_progress.lock().await;
        if !in_progress.insert(id) {
            return (StatusCode::ACCEPTED, Json(json!({"message":"Validasi pengembalian sedang diproses."})));
        }
    }
    let row = vec![
        Utc::now().to_rfc3339(),
        submitted.vehicle.clone(),
        submitted.return_name.clone().unwrap_or_default(),
        submitted.return_time.clone().unwrap_or_default(),
        submitted.fuel.clone().unwrap_or_default(),
        submitted.return_notes.clone().unwrap_or_default(),
    ];
    let background_state = state.clone();
    tokio::spawn(async move {
        let result = sheets_append("pengembalian", row).await;
        let mut app = background_state.lock().await;
        if let Some(booking) = app.bookings.iter_mut().find(|booking| booking.id == id && booking.status == "return_pending") {
            match result {
                Ok(()) => {
                    booking.status = "returned".into();
                    booking.return_validation_error = None;
                }
                Err(error) => {
                    eprintln!("Gagal menyimpan pengembalian yang divalidasi ke spreadsheet: {error}");
                    booking.return_validation_error = Some("Gagal menyimpan ke spreadsheet. Periksa koneksi lalu coba validasi lagi.".into());
                }
            }
            save_state(&app).await;
        }
        in_progress.lock().await.remove(&id);
    });
    (StatusCode::ACCEPTED, Json(json!({"message":"Validasi diproses. Kendaraan tersedia setelah spreadsheet berhasil diperbarui."})))
}

async fn create_booking(State(state): State<SharedState>, Json(input): Json<BookingInput>) -> (StatusCode, Json<Value>) {
    let mut app = state.lock().await;
    let vehicle_id = input.vehicle_id.unwrap_or_default();
    let vehicle = app.vehicles.iter().find(|v| v.id == vehicle_id).cloned();
    let Some(vehicle) = vehicle else { return (StatusCode::BAD_REQUEST, Json(json!({"error":"Kendaraan tidak ditemukan."}))); };
    let name = input.name.unwrap_or_default().trim().to_string();
    let loan_time = input.loan_time.unwrap_or_default();
    if name.is_empty() || loan_time.is_empty() { return (StatusCode::BAD_REQUEST, Json(json!({"error":"Nama dan waktu peminjaman wajib diisi."}))); }
    let loan_type = input.loan_type.unwrap_or_default();
    if !matches!(loan_type.as_str(), "full_day" | "half_day") {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"Pilih durasi Full Day atau Half Day."})));
    }
    if app.bookings.iter().any(|b| b.vehicle_id == vehicle_id && matches!(b.status.as_str(), "booked" | "return_pending")) { return (StatusCode::CONFLICT, Json(json!({"error":"Mobil sedang dipinjam."}))); }
    let booking = Booking {
        id: Uuid::new_v4(),
        vehicle_id: vehicle.id.clone(),
        vehicle: format!("{} {}", vehicle.name, vehicle.plate).trim().into(),
        name,
        nomor_st: input.nomor_st.unwrap_or_default().trim().to_string(),
        loan_time,
        notes: input.notes.unwrap_or_default(),
        status: "booked".into(),
        loan_type: Some(loan_type),
        created_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        check_in_time: None,
        check_out_time: None,
        return_name: None,
        return_time: None,
        fuel: None,
        return_notes: None,
        return_validation_error: None,
    };
    app.bookings.push(booking.clone());
    let response = json!({"booking": booking.clone(), "vehicles": snapshot(&app)["vehicles"]});
    save_state(&app).await;
    drop(app);
    tokio::spawn(async move {
        if let Err(error) = sheets_append_booking(&booking).await {
            eprintln!("Gagal menyimpan peminjaman ke spreadsheet: {error}");
        }
    });
    (StatusCode::CREATED, Json(response))
}
async fn check_in_booking(State(state): State<SharedState>, Path(id): Path<Uuid>, Json(_input): Json<EmptyInput>) -> (StatusCode, Json<Value>) {
    let mut app = state.lock().await;
    let Some(booking) = app.bookings.iter_mut().find(|booking| booking.id == id && booking.status == "booked") else {
        return (StatusCode::NOT_FOUND, Json(json!({"error":"Peminjaman tidak ditemukan."})));
    };
    if booking.check_in_time.is_some() {
        return (StatusCode::CONFLICT, Json(json!({"error":"Check-in sudah dicatat."})));
    }
    booking.check_in_time = Some(Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true));
    let updated = booking.clone();
    let response = json!({"vehicles": snapshot(&app)["vehicles"]});
    save_state(&app).await;
    drop(app);
    tokio::spawn(async move {
        if let Err(error) = sheets_update_booking_check_time(&updated, true).await {
            eprintln!("Gagal memperbarui waktu check-in di spreadsheet: {error}");
        }
    });
    (StatusCode::OK, Json(response))
}
async fn return_booking(State(state): State<SharedState>, Path(id): Path<Uuid>, Json(input): Json<ReturnInput>) -> (StatusCode, Json<Value>) {
    let mut app = state.lock().await;
    let Some(booking) = app.bookings.iter_mut().find(|b| b.id == id && b.status == "booked") else { return (StatusCode::NOT_FOUND, Json(json!({"error":"Peminjaman tidak ditemukan."}))); };
    if booking.check_in_time.is_none() {
        return (StatusCode::CONFLICT, Json(json!({"error":"Check-in harus dicatat sebelum mengajukan check-out."})));
    }
    let name = input.name.unwrap_or_default().trim().to_string();
    let return_time = input.return_time.unwrap_or_default();
    let fuel = input.fuel.unwrap_or_default().trim().to_string();
    if name.is_empty() || return_time.is_empty() || fuel.is_empty() { return (StatusCode::BAD_REQUEST, Json(json!({"error":"Nama, waktu pengembalian, dan pengisian bensin wajib diisi."}))); }
    booking.return_name=Some(name); booking.return_time=Some(return_time.clone()); booking.check_out_time=Some(return_time); booking.fuel=Some(fuel); booking.return_notes=Some(input.notes.unwrap_or_default()); booking.status="return_pending".into();
    let submitted = booking.clone();
    let response = json!({"booking": submitted.clone(), "vehicles": snapshot(&app)["vehicles"]});
    save_state(&app).await;
    drop(app);
    tokio::spawn(async move {
        if let Err(error) = sheets_update_booking_check_time(&submitted, false).await {
            eprintln!("Gagal memperbarui check-out pengembalian di spreadsheet: {error}");
        }
    });
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
    tokio::spawn(async {
        let (counts, employees) = tokio::join!(
            cached_sheets_booking_counts(),
            cached_sheets_employee_names()
        );
        if let Err(error) = counts {
            eprintln!("Gagal menghangatkan cache frekuensi spreadsheet: {error}");
        }
        if let Err(error) = employees {
            eprintln!("Gagal menghangatkan cache nama pegawai: {error}");
        }
    });
    let protected_api = Router::new()
        .route("/api/vehicles", get(state_response))
        .route("/api/employees", get(employee_names))
        .route("/api/returns/pending", get(pending_returns))
        .route("/api/bookings/reset", post(reset_bookings))
        .route("/api/logout", post(logout))
        .route("/api/bookings", post(create_booking))
        .route("/api/bookings/{id}/check-in", post(check_in_booking))
        .route("/api/bookings/{id}/return", post(return_booking))
        .route("/api/bookings/{id}/validate-return", post(validate_return))
        .route("/api/finance/entries", get(finance_entries).post(create_finance_entry).delete(reset_finance_entries))
        .route("/api/finance/chat", post(finance_chat))
        .route("/api/finance/summary", get(finance_summary_route))
        .route("/api/finance/daily", get(finance_daily))
        .route("/api/finance/export.csv", get(finance_export))
        .route_layer(middleware::from_fn(authenticate));
    let session_api = Router::new()
        .route("/api/session", get(current_session))
        .route_layer(middleware::from_fn(authenticate));
    let app = Router::new()
        .route("/api/login", post(login))
        .merge(session_api)
        .merge(protected_api)
        .route_layer(middleware::from_fn(no_cache_api))
        .fallback(static_file)
        .with_state(state);
    let addr = SocketAddr::from(([0,0,0,0], port));
    println!("Aplikasi keuangan dan fleet berjalan di http://localhost:{port}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::{
        booking_timing, classify_known_finance_entry, detect_finance_entry, local_entry_date,
        parse_chat_amount, spreadsheet_booking_count, vehicles, Booking, EntryType, FinanceEntry,
        LearnedKeyword,
    };
    use chrono::NaiveDate;
    use std::collections::HashMap;

    fn booking(loan_type: &str, check_in: &str, check_out: Option<&str>) -> Booking {
        Booking {
            id: uuid::Uuid::nil(),
            vehicle_id: "innova-1321".into(),
            vehicle: "Innova DG 1321 WP".into(),
            name: "Test".into(),
            nomor_st: String::new(),
            loan_time: "2026-10-15T08:00".into(),
            notes: String::new(),
            status: "booked".into(),
            loan_type: Some(loan_type.into()),
            created_at: "2026-10-15T00:00:00Z".into(),
            check_in_time: Some(check_in.into()),
            check_out_time: check_out.map(str::to_string),
            return_name: None,
            return_time: None,
            fuel: None,
            return_notes: None,
            return_validation_error: None,
        }
    }

    #[test]
    fn booking_duration_uses_full_day_and_half_day_limits() {
        let end = "2026-10-15T05:01:00Z";
        let now = NaiveDate::parse_from_str("2026-10-15", "%Y-%m-%d")
            .unwrap().and_hms_opt(5, 1, 0).unwrap();
        assert!(!booking_timing(&booking("full_day", "2026-10-15T00:00:00Z", Some(end)), now).1);
        assert!(booking_timing(&booking("half_day", "2026-10-15T00:00:00Z", Some(end)), now).1);
    }

    #[test]
    fn spreadsheet_vehicle_count_matches_name_and_plate() {
        let mut counts = HashMap::new();
        counts.insert("Innova DG 1321 WP".to_string(), 3);
        assert_eq!(spreadsheet_booking_count(&vehicles()[0], &counts), 3);
    }

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
