use serde::Deserialize;
use std::{cell::RefCell, collections::HashSet, rc::Rc};
use wasm_bindgen::{closure::Closure, prelude::*, JsCast};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{Document, Element, Event, HtmlFormElement, HtmlInputElement, HtmlSelectElement, Request, RequestInit, RequestMode, Response, Storage};

#[derive(Clone, Deserialize)]
struct Vehicle {
    id: String,
    name: String,
    plate: String,
    color: String,
    icon: String,
    status: String,
    #[serde(rename = "bookingCount", default)]
    booking_count: usize,
    booking: Option<Booking>,
}

#[derive(Clone, Deserialize)]
struct Booking {
    id: String,
    vehicle: String,
    name: String,
    #[serde(rename = "loanTime")]
    loan_time: String,
    #[serde(rename = "nomorST", default)]
    nomor_st: String,
    #[serde(rename = "loanType", default)]
    loan_type: Option<String>,
    #[serde(rename = "checkInTime", default)]
    check_in_time: Option<String>,
    #[serde(rename = "checkOutTime", default)]
    check_out_time: Option<String>,
    #[serde(rename = "elapsedMinutes", default)]
    elapsed_minutes: Option<i64>,
    #[serde(rename = "overLimit", default)]
    over_limit: bool,
    #[serde(default)]
    status: String,
    #[serde(rename = "returnName", default)]
    return_name: Option<String>,
    #[serde(rename = "returnTime", default)]
    return_time: Option<String>,
    #[serde(default)]
    fuel: Option<String>,
    #[serde(rename = "returnNotes", default)]
    return_notes: Option<String>,
    #[serde(rename = "returnValidationError", default)]
    return_validation_error: Option<String>,
    notes: String,
}

#[derive(Deserialize)]
struct UserSession {
    role: String,
}

#[derive(Deserialize)]
struct FleetResponse {
    vehicles: Vec<Vehicle>,
    #[serde(rename = "bookingCountError", default)]
    booking_count_error: Option<String>,
}

fn document() -> Document {
    web_sys::window().unwrap().document().unwrap()
}

fn el(id: &str) -> Element {
    document().get_element_by_id(id).unwrap()
}

fn esc(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
        .replace('"', "&quot;").replace('\'', "&#039;")
}

fn set_toast(message: &str) {
    let toast = el("toast");
    toast.set_text_content(Some(message));
    toast.set_class_name("toast show");
    let toast_clone = toast.clone();
    let callback = Closure::once(move || toast_clone.set_class_name("toast"));
    web_sys::window().unwrap()
        .set_timeout_with_callback_and_timeout_and_arguments_0(callback.as_ref().unchecked_ref(), 3500)
        .unwrap();
    callback.forget();
}

fn close_modal() {
    el("backdrop").set_class_name("modal-backdrop");
}

fn open_modal() {
    el("backdrop").set_class_name("modal-backdrop open");
}

fn format_date(value: &str) -> String {
    value.replace('T', " ").trim_end_matches('Z').to_string()
}

fn format_elapsed(minutes: i64) -> String {
    if minutes < 60 {
        format!("{minutes} menit")
    } else {
        format!("{} jam {} menit", minutes / 60, minutes % 60)
    }
}

fn form_field(label: &str, name: &str, input_type: &str, required: bool, placeholder: &str) -> String {
    format!(
        r#"<div class="form-row"><label for="{name}">{label}{}</label><input id="{name}" name="{name}" type="{input_type}" {} placeholder="{placeholder}"></div>"#,
        if required { " *" } else { "" },
        if required { "required" } else { "" }
    )
}

fn employee_select_field(label: &str, employees: &[String]) -> String {
    let options = employees.iter().map(|employee| {
        format!(r#"<option value="{}">{}</option>"#, esc(employee), esc(employee))
    }).collect::<String>();
    format!(
        r#"<div class="form-row"><label for="name">{label} *</label><select id="name" name="name" required><option value="">Pilih nama pegawai</option>{options}</select></div>"#
    )
}

fn render_frequency(vehicles: &[Vehicle]) {
    let total: usize = vehicles.iter().map(|vehicle| vehicle.booking_count).sum();
    let busiest = vehicles.iter().filter(|vehicle| vehicle.booking_count > 0)
        .max_by_key(|vehicle| vehicle.booking_count);
    let busiest_name = busiest.map(|vehicle| esc(&vehicle.name)).unwrap_or_else(|| "Belum ada data".into());
    let busiest_count = busiest.map(|vehicle| vehicle.booking_count).unwrap_or_default();
    el("bookingMetrics").set_inner_html(&format!(
        r#"<div class="metric"><span>Total peminjaman</span><strong>{total}</strong><small>seluruh kendaraan · kali</small></div><div class="metric"><span>Paling sering dipinjam</span><strong>{busiest_name}</strong><small>{busiest_count} kali</small></div>"#
    ));
    let mut ranked = vehicles.iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.booking_count.cmp(&left.booking_count));
    let peak = ranked.first().map(|vehicle| vehicle.booking_count).unwrap_or_default();
    if peak == 0 {
        el("bookingFrequency").set_inner_html(
            r#"<p class="frequency-empty">Belum ada riwayat peminjaman untuk ditampilkan.</p>"#,
        );
        return;
    }
    let rows = ranked.iter().enumerate().map(|(index, vehicle)| {
        format!(
            r#"<article class="frequency-tile"><span class="frequency-tile-rank">#{:02}</span><span class="frequency-tile-icon" aria-hidden="true">{}</span><strong class="frequency-tile-name">{}</strong><span class="frequency-tile-plate">{}</span><b class="frequency-tile-count">{}</b><small class="frequency-tile-unit">kali dipinjam</small><div class="frequency-tile-track"><i style="width:{:.1}%"></i></div></article>"#,
            index + 1, esc(&vehicle.icon), esc(&vehicle.name), esc(&vehicle.plate),
            vehicle.booking_count, vehicle.booking_count as f64 / peak as f64 * 100.0
        )
    }).collect::<String>();
    el("bookingFrequency").set_inner_html(&rows);
}

fn render(state: Rc<RefCell<Vec<Vehicle>>>) {
    let is_admin = document().body().unwrap().class_list().contains("admin");
    let vehicles = state.borrow();
    let available = vehicles.iter().filter(|vehicle| vehicle.status == "available").count();
    el("availableCount").set_text_content(Some(&available.to_string()));
    render_frequency(&vehicles);
    let cards = vehicles.iter().map(|vehicle| {
        let info = vehicle.booking.as_ref().map(|booking| {
            let (loan_type, limit) = match booking.loan_type.as_deref() {
                Some("full_day") => ("Full Day", "9,5 jam"),
                Some("half_day") => ("Half Day", "5 jam"),
                _ => ("Durasi tidak diketahui", "-"),
            };
            let check_time = booking.check_in_time.as_deref().map(|check_in| {
                let check_out = booking.check_out_time.as_deref()
                    .map(|time| format!(" · Check-out: {}", format_date(time)))
                    .unwrap_or_default();
                format!(
                    r#"<br>Check-in: {} · Durasi: {}{}"#,
                    format_date(check_in),
                    booking.elapsed_minutes.map(format_elapsed).unwrap_or_else(|| "belum dihitung".into()),
                    check_out
                )
            }).unwrap_or_default();
            let warning = if booking.over_limit {
                r#"<br><strong class="duration-warning">Segera dikembalikan — batas waktu terlampaui.</strong>"#
            } else {
                ""
            };
            let check_action = if is_admin {
                String::new()
            } else if booking.status == "return_pending" {
                r#"<span class="check-complete">Menunggu validasi admin</span>"#.into()
            } else if booking.check_in_time.is_none() {
                format!(r#"<button class="check-control" data-action="check-in" data-id="{}" type="button">Check-in</button>"#, booking.id)
            } else if booking.check_out_time.is_none() {
                format!(r#"<button class="check-control" data-action="check-out" data-id="{}" type="button">Check-out</button>"#, booking.id)
            } else {
                format!(r#"<button class="check-control" data-action="check-out" data-id="{}" type="button">{}</button>"#, booking.id, if booking.check_out_time.is_some() { "Lengkapi pengajuan" } else { "Check-out" })
            };
            format!(
                r#"<div class="booked-info"><span>Dipinjam oleh <strong>{}</strong></span><button class="view-booking" data-id="{}" title="Lihat detail peminjaman" aria-label="Lihat detail peminjaman">👁</button><br>{}<br>{} · Batas {}{}{warning}<div class="check-actions">{check_action}</div></div>"#,
                esc(&booking.name), vehicle.id, format_date(&booking.loan_time), loan_type, limit, check_time
            )
        }).unwrap_or_default();
        format!(
            r#"<article class="vehicle" data-id="{}" data-color="{}" data-status="{}" tabindex="0" aria-label="{}: {}"><div class="vehicle-img"><span class="vehicle-icon" role="img" aria-label="{}">{}</span></div><div class="vehicle-top"><div><h3>{}</h3><span class="plate">Nopol {}</span></div><span class="status {}">{}</span></div>{}<div class="vehicle-action">{}</div></article>"#,
            vehicle.id, vehicle.color, vehicle.status, esc(&vehicle.name),
            if vehicle.status == "available" { "tersedia" } else { "sedang dipinjam" },
            esc(&vehicle.name), esc(&vehicle.icon), esc(&vehicle.name), esc(&vehicle.plate),
            vehicle.status, if vehicle.status == "available" { "Tersedia" } else { "Dipinjam" },
            info, if is_admin { "Panel admin" } else if vehicle.status == "available" { "＋ Catat peminjaman" } else { "" }
        )
    }).collect::<String>();
    el("fleet").set_inner_html(&cards);
}

async fn post_json(endpoint: &str, body: &str) -> Result<serde_json::Value, String> {
    let init = RequestInit::new();
    init.set_method("POST");
    init.set_mode(RequestMode::SameOrigin);
    init.set_body(&JsValue::from_str(body));
    let request = Request::new_with_str_and_init(endpoint, &init)
        .map_err(|error| format!("{error:?}"))?;
    request.headers().set("Cache-Control", "no-cache")
        .map_err(|error| format!("{error:?}"))?;
    request.headers().set("Content-Type", "application/json")
        .map_err(|error| format!("{error:?}"))?;
    if let Some(token) = auth_token() {
        request.headers().set("Authorization", &format!("Bearer {token}"))
            .map_err(|error| format!("{error:?}"))?;
    }
    let response = JsFuture::from(web_sys::window().unwrap().fetch_with_request(&request)).await
        .map_err(|_| "Server belum tersedia.".to_string())?
        .dyn_into::<Response>().map_err(|_| "Respons server tidak valid.".to_string())?;
    let body = JsFuture::from(response.json().map_err(|_| "Respons JSON tidak valid.".to_string())?)
        .await.map_err(|_| "Respons JSON tidak dapat dibaca.".to_string())?;
    let result: serde_json::Value = serde_wasm_bindgen::from_value(body)
        .map_err(|error| format!("Respons server tidak valid: {error}"))?;
    if !response.ok() {
        return Err(result["error"].as_str().unwrap_or("Terjadi kesalahan.").to_string());
    }
    Ok(result)
}

fn storage() -> Result<Storage, String> {
    web_sys::window().unwrap().local_storage()
        .map_err(|_| "Penyimpanan sesi tidak tersedia.".to_string())?
        .ok_or_else(|| "Penyimpanan sesi tidak tersedia.".to_string())
}

fn auth_token() -> Option<String> {
    storage().ok()?.get_item("fleetAuthToken").ok().flatten()
}

fn set_auth_token(token: Option<&str>) -> Result<(), String> {
    let storage = storage()?;
    if let Some(token) = token {
        storage.set_item("fleetAuthToken", token)
            .map_err(|_| "Sesi login tidak dapat disimpan.".to_string())
    } else {
        storage.remove_item("fleetAuthToken")
            .map_err(|_| "Sesi login tidak dapat dihapus.".to_string())
    }
}

async fn get_json(endpoint: &str) -> Result<serde_json::Value, String> {
    let init = RequestInit::new();
    init.set_method("GET");
    init.set_mode(RequestMode::SameOrigin);
    let request = Request::new_with_str_and_init(endpoint, &init)
        .map_err(|error| format!("{error:?}"))?;
    if let Some(token) = auth_token() {
        request.headers().set("Authorization", &format!("Bearer {token}"))
            .map_err(|error| format!("{error:?}"))?;
    }
    let response = JsFuture::from(web_sys::window().unwrap().fetch_with_request(&request)).await
        .map_err(|_| "Tidak dapat terhubung ke server.".to_string())?
        .dyn_into::<Response>().map_err(|_| "Respons server tidak valid.".to_string())?;
    let body = JsFuture::from(response.json().map_err(|_| "Respons JSON tidak valid.".to_string())?)
        .await.map_err(|_| "Respons JSON tidak dapat dibaca.".to_string())?;
    let result: serde_json::Value = serde_wasm_bindgen::from_value(body)
        .map_err(|error| format!("Respons server tidak valid: {error}"))?;
    if !response.ok() {
        return Err(result["error"].as_str().unwrap_or("Terjadi kesalahan.").to_string());
    }
    Ok(result)
}

async fn fetch_employee_names() -> Result<Vec<String>, String> {
    serde_json::from_value::<Vec<String>>(get_json("/api/employees").await?)
        .map_err(|error| format!("Format daftar pegawai tidak sesuai: {error}"))
}

async fn load_employee_names(cache: &Rc<RefCell<Option<Vec<String>>>>) -> Result<Vec<String>, String> {
    if let Some(names) = cache.borrow().as_ref() {
        return Ok(names.clone());
    }
    let names = fetch_employee_names().await?;
    *cache.borrow_mut() = Some(names.clone());
    Ok(names)
}

async fn refresh_fleet(state: Rc<RefCell<Vec<Vehicle>>>) -> Result<(), String> {
    let result: FleetResponse = serde_json::from_value(get_json("/api/vehicles").await?)
        .map_err(|error| format!("Format data kendaraan tidak sesuai: {error}"))?;
    *state.borrow_mut() = result.vehicles;
    render(state);
    if let Some(error) = result.booking_count_error {
        el("bookingMetrics").set_text_content(Some(&error));
        el("bookingFrequency").set_text_content(None);
    }
    Ok(())
}

async fn refresh_pending_returns(validating: Rc<RefCell<HashSet<String>>>) -> Result<(), String> {
    let bookings: Vec<Booking> = serde_json::from_value(get_json("/api/returns/pending").await?)
        .map_err(|error| format!("Daftar validasi tidak sesuai: {error}"))?;
    {
        let mut active = validating.borrow_mut();
        active.retain(|id| bookings.iter().any(|booking| &booking.id == id && booking.return_validation_error.is_none()));
    }
    if bookings.is_empty() {
        el("pendingReturns").set_inner_html(r#"<p class="pending-empty">Belum ada pengajuan check-out untuk divalidasi.</p>"#);
        return Ok(());
    }
    let active = validating.borrow().clone();
    let cards = bookings.iter().map(|booking| {
        let error = booking.return_validation_error.as_deref()
            .map(|message| format!(r#"<p class="validation-error">{}</p>"#, esc(message)))
            .unwrap_or_default();
        let button = if active.contains(&booking.id) {
            "disabled aria-busy=\"true\">Sedang memvalidasi…"
        } else {
            " >Validasi pengembalian"
        };
        format!(
            r#"<article class="pending-card"><h3>{} · Dipinjam oleh {}</h3><div class="booking-detail"><div><span>Waktu peminjaman</span><strong>{}</strong></div><div><span>Nomor ST</span><strong>{}</strong></div><div><span>Tujuan</span><strong>{}</strong></div><div><span>Check-in</span><strong>{}</strong></div><div><span>Waktu check-out</span><strong>{}</strong></div><div><span>Nama pengembali</span><strong>{}</strong></div><div><span>Pengisian bensin</span><strong>{}</strong></div><div><span>Keterangan pengembalian</span><strong>{}</strong></div></div>{error}<button class="submit validate-return" data-id="{}" type="button" {button}</button></article>"#,
            esc(&booking.vehicle), esc(&booking.name), format_date(&booking.loan_time),
            esc(&booking.nomor_st), esc(&booking.notes),
            booking.check_in_time.as_deref().map(format_date).unwrap_or_else(|| "Belum check-in".into()),
            booking.return_time.as_deref().map(format_date).unwrap_or_else(|| "Belum diisi".into()),
            esc(booking.return_name.as_deref().unwrap_or("Belum diisi")),
            esc(booking.fuel.as_deref().unwrap_or("Belum diisi")),
            esc(booking.return_notes.as_deref().unwrap_or("")),
            esc(&booking.id)
        )
    }).collect::<String>();
    el("pendingReturns").set_inner_html(&cards);
    Ok(())
}

fn validate_return(booking_id: String, validating: Rc<RefCell<HashSet<String>>>) {
    validating.borrow_mut().insert(booking_id.clone());
    let active = validating.clone();
    spawn_local(async move {
        match post_json(&format!("/api/bookings/{booking_id}/validate-return"), "{}").await {
            Ok(_) => {
                set_toast("Validasi diproses");
                if let Err(error) = refresh_pending_returns(active).await {
                    set_toast(&error);
                }
            }
            Err(error) => {
                validating.borrow_mut().remove(&booking_id);
                set_toast(&error);
                if let Err(refresh_error) = refresh_pending_returns(validating).await {
                    set_toast(&refresh_error);
                }
            }
        }
    });
}

fn input_value(form: &HtmlFormElement, name: &str) -> String {
    form.elements().named_item(name).unwrap().dyn_into::<HtmlInputElement>().unwrap().value()
}

fn selected_value(form: &HtmlFormElement, name: &str) -> String {
    form.elements().named_item(name).unwrap().dyn_into::<HtmlSelectElement>().unwrap().value()
}

fn show_booking_form(vehicle: Vehicle, state: Rc<RefCell<Vec<Vehicle>>>, employees: &[String]) {
    el("modalContent").set_inner_html(&format!(
        r#"<h2>Pinjam {}</h2><p class="sub">Nopol {} · Pilih batas waktu peminjaman.</p><form id="bookingForm">{}{}{}<fieldset class="duration-options"><legend>Durasi peminjaman *</legend><label><input type="radio" name="loanType" value="full_day" required> Full Day (9,5 jam)</label><label><input type="radio" name="loanType" value="half_day" required> Half Day (5 jam)</label></fieldset>{}<button class="submit">Konfirmasi peminjaman</button></form>"#,
        esc(&vehicle.name), esc(&vehicle.plate),
        employee_select_field("Nama peminjam", employees),
        form_field("Nomor ST", "nomorST", "text", false, "Masukkan nomor ST"),
        form_field("Jam peminjaman", "loanTime", "datetime-local", true, ""),
        form_field("Keterangan", "notes", "text", false, "Tujuan perjalanan (opsional)")
    ));
    attach_form("bookingForm", "/api/bookings".into(), format!(r#"{{"vehicleId":"{}"}}"#, vehicle.id), "Peminjaman berhasil dicatat.", state);
}

fn show_booking(vehicle: Vehicle, state: Rc<RefCell<Vec<Vehicle>>>, employees: Rc<RefCell<Option<Vec<String>>>>) {
    open_modal();
    if let Some(names) = employees.borrow().as_ref() {
        show_booking_form(vehicle, state, names);
        return;
    }
    el("modalContent").set_text_content(Some("Memuat daftar pegawai..."));
    spawn_local(async move {
        match load_employee_names(&employees).await {
            Ok(names) if !names.is_empty() => show_booking_form(vehicle, state, &names),
            Ok(_) => show_employee_error("Daftar nama pegawai pada sheet pegawai masih kosong."),
            Err(error) => show_employee_error(&error),
        }
    });
}

fn show_return_form(vehicle: Vehicle, booking: Booking, state: Rc<RefCell<Vec<Vehicle>>>, employees: &[String]) {
    el("modalContent").set_inner_html(&format!(
        r#"<h2>Ajukan check-out {}</h2><p class="sub">Dipinjam oleh {} · {}</p><form id="returnForm">{}{}{}{}<button class="submit">Kirim untuk validasi admin</button></form>"#,
        esc(&vehicle.name), esc(&booking.name), format_date(&booking.loan_time),
        employee_select_field("Nama pengembali", employees),
        form_field("Jam pengembalian", "returnTime", "datetime-local", true, ""),
        form_field("Pengisian bensin", "fuel", "text", true, "Contoh: Rp150.000 / Full tank"),
        form_field("Keterangan", "notes", "text", false, "Catatan (opsional)")
    ));
    attach_form(
        "returnForm",
        format!("/api/bookings/{}/return", booking.id),
        "{}".into(),
        "Pengajuan check-out dikirim; kendaraan menunggu validasi admin.",
        state,
    );
}

fn show_return(vehicle: Vehicle, state: Rc<RefCell<Vec<Vehicle>>>, employees: Rc<RefCell<Option<Vec<String>>>>) {
    let booking = vehicle.booking.clone().unwrap();
    open_modal();
    if let Some(names) = employees.borrow().as_ref() {
        show_return_form(vehicle, booking, state, names);
        return;
    }
    el("modalContent").set_text_content(Some("Memuat daftar pegawai..."));
    spawn_local(async move {
        match load_employee_names(&employees).await {
            Ok(names) if !names.is_empty() => show_return_form(vehicle, booking, state, &names),
            Ok(_) => show_employee_error("Daftar nama pegawai pada sheet pegawai masih kosong."),
            Err(error) => show_employee_error(&error),
        }
    });
}

fn show_employee_error(error: &str) {
    el("modalContent").set_inner_html(&format!(
        "<h2>Daftar pegawai tidak tersedia</h2><p class=\"sub\">{} Perbarui dan deploy Apps Script agar pilihan nama dapat dimuat.</p>",
        esc(error)
    ));
}

fn show_booking_detail(vehicle: Vehicle) {
    let booking = vehicle.booking.unwrap();
    let loan_type = match booking.loan_type.as_deref() {
        Some("full_day") => "Full Day (9,5 jam)",
        Some("half_day") => "Half Day (5 jam)",
        _ => "Tidak diketahui",
    };
    el("modalContent").set_inner_html(&format!(
        r#"<h2>Detail peminjaman</h2><p class="sub">{} · Nopol {}</p><div class="booking-detail"><div><span>Nama peminjam</span><strong>{}</strong></div><div><span>Nomor ST</span><strong>{}</strong></div><div><span>Jam peminjaman</span><strong>{}</strong></div><div><span>Durasi</span><strong>{loan_type}</strong></div><div><span>Check-in</span><strong>{}</strong></div><div><span>Check-out</span><strong>{}</strong></div><div><span>Durasi aktual</span><strong>{}</strong></div><div><span>Keterangan</span><strong>{}</strong></div></div>"#,
        esc(&vehicle.name), esc(&vehicle.plate), esc(&booking.name), esc(&booking.nomor_st),
        format_date(&booking.loan_time),
        booking.check_in_time.as_deref().map(format_date).unwrap_or_else(|| "Belum check-in".into()),
        booking.check_out_time.as_deref().map(format_date).unwrap_or_else(|| "Belum check-out".into()),
        booking.elapsed_minutes.map(format_elapsed).unwrap_or_else(|| "Belum dihitung".into()),
        esc(&booking.notes)
    ));
    open_modal();
}

fn attach_form(id: &str, endpoint: String, extra: String, success: &str, state: Rc<RefCell<Vec<Vehicle>>>) {
    let form: HtmlFormElement = el(id).dyn_into().unwrap();
    let booking_form = id == "bookingForm";
    let callback = Closure::wrap(Box::new(move |event: Event| {
        event.prevent_default();
        let form = event.target().unwrap().dyn_into::<HtmlFormElement>().unwrap();
        let mut payload = serde_json::Map::new();
        payload.insert("name".into(), selected_value(&form, "name").into());
        payload.insert("notes".into(), input_value(&form, "notes").into());
        let (time_key, time) = if booking_form {
            ("loanTime", input_value(&form, "loanTime"))
        } else {
            ("returnTime", input_value(&form, "returnTime"))
        };
        let time = if booking_form {
            time
        } else {
            js_sys::Date::new(&JsValue::from_str(&time)).to_iso_string().as_string().unwrap_or_default()
        };
        payload.insert(time_key.into(), time.into());
        if booking_form {
            payload.insert("nomorST".into(), input_value(&form, "nomorST").into());
            let loan_type = form.query_selector(r#"input[name="loanType"]:checked"#).unwrap()
                .unwrap().dyn_into::<HtmlInputElement>().unwrap().value();
            payload.insert("loanType".into(), loan_type.into());
        } else {
            payload.insert("fuel".into(), input_value(&form, "fuel").into());
        }
        let extra_value: serde_json::Value = serde_json::from_str(&extra).expect("form extras must be valid JSON");
        if let Some(fields) = extra_value.as_object() {
            payload.extend(fields.clone());
        }
        let json = serde_json::Value::Object(payload).to_string();
        let endpoint = endpoint.clone();
        let state = state.clone();
        let success = success.to_string();
        spawn_local(async move {
            match post_json(&endpoint, &json).await {
                Ok(result) => {
                    match serde_json::from_value::<Vec<Vehicle>>(result["vehicles"].clone()) {
                        Ok(vehicles) => *state.borrow_mut() = vehicles,
                        Err(error) => {
                            set_toast(&format!("Respons armada tidak valid: {error}"));
                            return;
                        }
                    }
                    close_modal();
                    render(state.clone());
                    set_toast(&success);
                }
                Err(error) => set_toast(&error),
            }
        });
    }) as Box<dyn FnMut(_)>);
    form.add_event_listener_with_callback("submit", callback.as_ref().unchecked_ref()).unwrap();
    callback.forget();
}

fn show_check_action(action: String, booking_id: String, state: Rc<RefCell<Vec<Vehicle>>>) {
    spawn_local(async move {
        let endpoint = format!("/api/bookings/{booking_id}/{action}");
        match post_json(&endpoint, "{}").await {
            Ok(result) => match serde_json::from_value::<Vec<Vehicle>>(result["vehicles"].clone()) {
                Ok(vehicles) => {
                    *state.borrow_mut() = vehicles;
                    render(state);
                    set_toast("Check-in berhasil dicatat.");
                }
                Err(error) => set_toast(&format!("Respons armada tidak valid: {error}")),
            },
            Err(error) => set_toast(&error),
        }
    });
}

fn activate_session(session: UserSession, state: Rc<RefCell<Vec<Vehicle>>>, employee_names: Rc<RefCell<Option<Vec<String>>>>, validating_returns: Rc<RefCell<HashSet<String>>>) {
    let role = session.role;
    document().body().unwrap().set_class_name(if role == "admin" { "authenticated admin" } else { "authenticated user" });
    el("roleBadge").set_class_name(if role == "admin" { "status admin-role" } else { "status user-role" });
    el("roleBadge").set_text_content(Some(if role == "admin" { "Admin" } else { "User" }));
    el("loginError").set_text_content(None);

    let employee_cache = employee_names.clone();
    spawn_local(async move {
        if let Err(error) = load_employee_names(&employee_cache).await {
            web_sys::console::error_1(&JsValue::from_str(&error));
        }
    });

    let initial_state = state.clone();
    spawn_local(async move {
        if let Err(error) = refresh_fleet(initial_state).await {
            set_toast(&error);
        }
    });
    if role == "admin" {
        spawn_local(async move {
            if let Err(error) = refresh_pending_returns(validating_returns).await {
                set_toast(&error);
            }
        });
    }
}

#[wasm_bindgen(start)]
pub fn start() {
    let date = js_sys::Date::new_0();
    let months = ["Januari", "Februari", "Maret", "April", "Mei", "Juni", "Juli", "Agustus", "September", "Oktober", "November", "Desember"];
    let month = months.get(date.get_month() as usize).copied().unwrap_or("Januari");
    el("today").set_text_content(Some(&format!("{} {} {}", date.get_date(), month, date.get_full_year())));

    let state: Rc<RefCell<Vec<Vehicle>>> = Rc::new(RefCell::new(Vec::new()));
    let employee_names: Rc<RefCell<Option<Vec<String>>>> = Rc::new(RefCell::new(None));
    let validating_returns: Rc<RefCell<HashSet<String>>> = Rc::new(RefCell::new(HashSet::new()));
    let fleet_state = state.clone();
    let fleet_employee_names = employee_names.clone();
    let click = Closure::wrap(Box::new(move |event: Event| {
        let target: Element = event.target().unwrap().dyn_into().unwrap();
        if let Ok(Some(eye)) = target.closest(".view-booking") {
            event.stop_propagation();
            let id = eye.get_attribute("data-id").unwrap_or_default();
            if let Some(vehicle) = fleet_state.borrow().iter().find(|vehicle| vehicle.id == id).cloned() {
                show_booking_detail(vehicle);
            }
        } else if let Ok(Some(control)) = target.closest(".check-control") {
            event.stop_propagation();
            let action = control.get_attribute("data-action").unwrap_or_default();
            let id = control.get_attribute("data-id").unwrap_or_default();
            if action == "check-out" {
                if let Some(vehicle) = fleet_state.borrow().iter().find(|vehicle| vehicle.booking.as_ref().is_some_and(|booking| booking.id == id)).cloned() {
                    show_return(vehicle, fleet_state.clone(), fleet_employee_names.clone());
                }
            } else {
                show_check_action(action, id, fleet_state.clone());
            }
        } else if let Ok(Some(card)) = target.closest(".vehicle") {
            if document().body().unwrap().class_list().contains("admin") {
                return;
            }
            let id = card.get_attribute("data-id").unwrap_or_default();
            if let Some(vehicle) = fleet_state.borrow().iter().find(|vehicle| vehicle.id == id).cloned() {
                if vehicle.status == "available" {
                    show_booking(vehicle, fleet_state.clone(), fleet_employee_names.clone());
                }
            }
        }
    }) as Box<dyn FnMut(_)>);
    el("fleet").add_event_listener_with_callback("click", click.as_ref().unchecked_ref()).unwrap();
    click.forget();

    let backdrop = el("backdrop");
    let backdrop_copy = backdrop.clone();
    let backdrop_click = Closure::wrap(Box::new(move |event: Event| {
        let target = event.target().unwrap();
        let backdrop_target: web_sys::EventTarget = backdrop_copy.clone().unchecked_into();
        if target == backdrop_target {
            close_modal();
        }
    }) as Box<dyn FnMut(Event)>);
    backdrop.add_event_listener_with_callback("click", backdrop_click.as_ref().unchecked_ref()).unwrap();
    backdrop_click.forget();

    let close = Closure::wrap(Box::new(move |_event: Event| close_modal()) as Box<dyn FnMut(Event)>);
    el("closeModal").add_event_listener_with_callback("click", close.as_ref().unchecked_ref()).unwrap();
    close.forget();

    let approval_in_progress = validating_returns.clone();
    let approval_click = Closure::wrap(Box::new(move |event: Event| {
        let Ok(target) = event.target().unwrap().dyn_into::<Element>() else { return };
        if let Ok(Some(button)) = target.closest(".validate-return") {
            let id = button.get_attribute("data-id").unwrap_or_default();
            validate_return(id, approval_in_progress.clone());
        }
    }) as Box<dyn FnMut(Event)>);
    el("pendingReturns").add_event_listener_with_callback("click", approval_click.as_ref().unchecked_ref()).unwrap();
    approval_click.forget();

    let login_state = state.clone();
    let login_employees = employee_names.clone();
    let login_validations = validating_returns.clone();
    let login_form: HtmlFormElement = el("loginForm").dyn_into().unwrap();
    let login_submit = Closure::wrap(Box::new(move |event: Event| {
        event.prevent_default();
        let form: HtmlFormElement = event.target().unwrap().dyn_into().unwrap();
        let credentials = serde_json::json!({
            "username": input_value(&form, "username"),
            "password": input_value(&form, "password")
        }).to_string();
        let state = login_state.clone();
        let employee_names = login_employees.clone();
        let validating_returns = login_validations.clone();
        spawn_local(async move {
            match post_json("/api/login", &credentials).await {
                Ok(result) => {
                    let Some(token) = result["token"].as_str() else {
                        el("loginError").set_text_content(Some("Respons login tidak valid."));
                        return;
                    };
                    if let Err(error) = set_auth_token(Some(token)) {
                        el("loginError").set_text_content(Some(&error));
                        return;
                    }
                    let session = UserSession {
                        role: result["role"].as_str().unwrap_or_default().to_string(),
                    };
                    activate_session(session, state, employee_names, validating_returns);
                }
                Err(error) => el("loginError").set_text_content(Some(&error)),
            }
        });
    }) as Box<dyn FnMut(Event)>);
    login_form.add_event_listener_with_callback("submit", login_submit.as_ref().unchecked_ref()).unwrap();
    login_submit.forget();

    let logout_state = state.clone();
    let logout_employees = employee_names.clone();
    let logout_validations = validating_returns.clone();
    let logout = Closure::wrap(Box::new(move |_event: Event| {
        spawn_local(async {
            let _ = post_json("/api/logout", "{}").await;
            if let Err(error) = set_auth_token(None) {
                web_sys::console::error_1(&JsValue::from_str(&error));
            }
            document().body().unwrap().set_class_name("");
        });
        *logout_state.borrow_mut() = Vec::new();
        *logout_employees.borrow_mut() = None;
        logout_validations.borrow_mut().clear();
        el("fleet").set_inner_html("");
        el("pendingReturns").set_inner_html("");
    }) as Box<dyn FnMut(Event)>);
    el("logout").add_event_listener_with_callback("click", logout.as_ref().unchecked_ref()).unwrap();
    logout.forget();

    let initial_state = state.clone();
    let initial_employees = employee_names.clone();
    let initial_validations = validating_returns.clone();
    spawn_local(async move {
        if auth_token().is_some() {
            match get_json("/api/session").await.and_then(|value| {
                serde_json::from_value::<UserSession>(value)
                    .map_err(|error| format!("Sesi login tidak valid: {error}"))
            }) {
                Ok(session) => activate_session(session, initial_state, initial_employees, initial_validations),
                Err(_) => {
                    let _ = set_auth_token(None);
                    document().body().unwrap().set_class_name("");
                }
            }
        }
    });

    let poll_state = state.clone();
    let poll = Closure::wrap(Box::new(move || {
        if document().hidden() {
            return;
        }
        let state = poll_state.clone();
        spawn_local(async move {
            if document().body().unwrap().class_list().contains("authenticated") {
                if let Err(error) = refresh_fleet(state).await {
                    web_sys::console::error_1(&JsValue::from_str(&error));
                }
            }
        });
    }) as Box<dyn FnMut()>);
    web_sys::window().unwrap()
        .set_interval_with_callback_and_timeout_and_arguments_0(poll.as_ref().unchecked_ref(), 1_000)
        .unwrap();
    poll.forget();

    let pending_validations = validating_returns.clone();
    let pending_poll = Closure::wrap(Box::new(move || {
        if document().hidden() || !document().body().unwrap().class_list().contains("admin") {
            return;
        }
        let validating = pending_validations.clone();
        spawn_local(async move {
            if let Err(error) = refresh_pending_returns(validating).await {
                web_sys::console::error_1(&JsValue::from_str(&error));
            }
        });
    }) as Box<dyn FnMut()>);
    web_sys::window().unwrap()
        .set_interval_with_callback_and_timeout_and_arguments_0(pending_poll.as_ref().unchecked_ref(), 2_500)
        .unwrap();
    pending_poll.forget();
}
