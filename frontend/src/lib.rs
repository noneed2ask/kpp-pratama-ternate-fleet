use serde::Deserialize;
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{closure::Closure, prelude::*, JsCast};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{Document, Element, Event, HtmlFormElement, HtmlInputElement, Request, RequestInit, RequestMode, Response};

#[derive(Clone, Deserialize)]
struct Vehicle {
    id: String, name: String, plate: String, color: String, icon: String, status: String,
    booking: Option<Booking>,
}
#[derive(Clone, Deserialize)]
struct Booking {
    id: String, name: String, #[serde(rename = "loanTime")] loan_time: String,
    notes: String,
}
#[derive(Deserialize)]
struct FleetResponse { vehicles: Vec<Vehicle> }

fn document() -> Document { web_sys::window().unwrap().document().unwrap() }
fn el(id: &str) -> Element { document().get_element_by_id(id).unwrap() }
fn esc(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#039;")
}
fn form_field(label: &str, name: &str, input_type: &str, required: bool, placeholder: &str) -> String {
    format!(r#"<div class="form-row"><label for="{name}">{label}{} </label><input id="{name}" name="{name}" type="{input_type}" {} placeholder="{placeholder}"></div>"#,
        if required {" *"} else {""}, if required {"required"} else {""})
}
fn set_toast(message: &str) {
    let toast = el("toast"); toast.set_text_content(Some(message)); toast.set_class_name("toast show");
    let toast_clone = toast.clone();
    let callback = Closure::once(move || toast_clone.set_class_name("toast"));
    web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(callback.as_ref().unchecked_ref(), 3000).unwrap();
    callback.forget();
}
fn close_modal() { el("backdrop").set_class_name("modal-backdrop"); }
fn open_modal() { el("backdrop").set_class_name("modal-backdrop open"); }
fn format_date(value: &str) -> String { value.replace('T', " ") }
fn car_svg(color: &str) -> String {
    let body = match color {
        "black" => "#252c32",
        "silver" => "#aab4bc",
        _ => "#0068a9",
    };
    format!(r##"<svg class="car-svg" viewBox="0 0 280 150" role="img" aria-label="Mobil tampak samping">
      <defs>
        <linearGradient id="body-{color}" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stop-color="{body}" stop-opacity=".98"/>
          <stop offset="1" stop-color="{body}" stop-opacity=".78"/>
        </linearGradient>
      </defs>
      <path d="M24 101c0-7 5-13 12-15l25-7 25-37c4-6 10-9 18-9h54c9 0 16 3 22 10l29 36 36 8c7 2 11 8 11 15v9H24v-10Z" fill="url(#body-{color})" stroke="#17222a" stroke-width="3" stroke-linejoin="round"/>
      <path d="M91 76 108 45c2-4 5-6 10-6h37c6 0 10 2 14 7l22 30H91Z" fill="#dce9ef" stroke="#17222a" stroke-width="3" stroke-linejoin="round"/>
      <path d="m111 43-16 30h43V43h-27Zm34 0v30h39l-22-30h-17Z" fill="#7894a3" opacity=".68"/>
      <path d="M25 94h28M227 94h27" stroke="#dbe7eb" stroke-width="5" stroke-linecap="round"/>
      <path d="M56 84h165" stroke="#fff" stroke-opacity=".34" stroke-width="3"/>
      <rect x="24" y="91" width="18" height="10" rx="4" fill="#f3c85b"/>
      <rect x="239" y="91" width="16" height="10" rx="4" fill="#e77865"/>
      <circle cx="77" cy="111" r="22" fill="#1c252b" stroke="#11181d" stroke-width="3"/>
      <circle cx="77" cy="111" r="10" fill="#c8d1d5"/><circle cx="77" cy="111" r="4" fill="#6c7c84"/>
      <circle cx="207" cy="111" r="22" fill="#1c252b" stroke="#11181d" stroke-width="3"/>
      <circle cx="207" cy="111" r="10" fill="#c8d1d5"/><circle cx="207" cy="111" r="4" fill="#6c7c84"/>
      <path d="M41 105h22M220 105h22" stroke="#17222a" stroke-width="4" stroke-linecap="round"/>
    </svg>"##
    )
}

fn render(state: Rc<RefCell<Vec<Vehicle>>>) {
    let vehicles = state.borrow();
    let available = vehicles.iter().filter(|v| v.status == "available").count();
    el("availableCount").set_text_content(Some(&available.to_string()));
    let cards = vehicles.iter().map(|v| {
        let info = v.booking.as_ref().map(|b| format!(r#"<div class="booked-info"><span>Dipinjam oleh <strong>{}</strong></span><button class="view-booking" data-id="{}" title="Lihat detail peminjaman" aria-label="Lihat detail peminjaman">👁</button><br>{}</div>"#, esc(&b.name), v.id, format_date(&b.loan_time))).unwrap_or_default();
        format!(r#"<article class="vehicle" data-id="{}" data-color="{}"><div class="vehicle-img"><span class="vehicle-icon">{}</span></div><div class="vehicle-top"><div><h3>{}</h3><span class="plate">Nopol {}</span></div><span class="status {}">{}</span></div>{}</article>"#,
            v.id, v.color, v.icon, v.name, v.plate, v.status, if v.status == "available" {"Tersedia"} else {"Dipinjam"}, info)
    }).collect::<String>();
    el("fleet").set_inner_html(&cards);
}

fn input_value(form: &HtmlFormElement, name: &str) -> String {
    form.elements().named_item(name).unwrap().dyn_into::<HtmlInputElement>().unwrap().value()
}
fn show_booking(vehicle: Vehicle, state: Rc<RefCell<Vec<Vehicle>>>) {
    el("modalContent").set_inner_html(&format!(r#"<h2>Pinjam {}</h2><p class="sub">Nopol {} · Pastikan data diisi dengan benar.</p><form id="bookingForm">{}{}{}<button class="submit">Konfirmasi peminjaman</button></form>"#,
        vehicle.name, vehicle.plate, form_field("Nama peminjam", "name", "text", true, "Masukkan nama lengkap"), form_field("Jam peminjaman", "loanTime", "datetime-local", true, ""), form_field("Keterangan", "notes", "text", false, "Tujuan perjalanan (opsional)")));
    attach_form("bookingForm", format!("/api/bookings"), format!(r#"{{"vehicleId":"{}"}}"#, vehicle.id), "Peminjaman berhasil dicatat.", state);
    open_modal();
}
fn show_return(vehicle: Vehicle, state: Rc<RefCell<Vec<Vehicle>>>) {
    let booking = vehicle.booking.unwrap();
    el("modalContent").set_inner_html(&format!(r#"<h2>Kembalikan {}</h2><p class="sub">Dipinjam oleh {} · {}</p><form id="returnForm">{}{}{}{}<button class="submit">Konfirmasi pengembalian</button></form>"#,
        vehicle.name, esc(&booking.name), format_date(&booking.loan_time), form_field("Nama pengembali", "name", "text", true, "Masukkan nama lengkap"), form_field("Jam pengembalian", "returnTime", "datetime-local", true, ""), form_field("Pengisian bensin", "fuel", "text", true, "Contoh: Rp150.000 / Full tank"), form_field("Keterangan", "notes", "text", false, "Catatan (opsional)")));
    attach_form(&"returnForm", format!("/api/bookings/{}/return", booking.id), "{}".into(), "Pengembalian berhasil dicatat.", state);
    open_modal();
}
fn show_booking_detail(vehicle: Vehicle) {
    let booking = vehicle.booking.unwrap();
    el("modalContent").set_inner_html(&format!(r#"
        <h2>Detail peminjaman</h2>
        <p class="sub">{} · Nopol {}</p>
        <div class="booking-detail">
          <div><span>Nama peminjam</span><strong>{}</strong></div>
          <div><span>Jam peminjaman</span><strong>{}</strong></div>
          <div><span>Keterangan</span><strong>{}</strong></div>
        </div>
        "#,
        vehicle.name, vehicle.plate, esc(&booking.name), format_date(&booking.loan_time), esc(&booking.notes)));
    open_modal();
}
fn attach_form(id: &str, endpoint: String, extra: String, success: &str, state: Rc<RefCell<Vec<Vehicle>>>) {
    let form: HtmlFormElement = el(id).dyn_into().unwrap();
    let booking_form = id == "bookingForm";
    let callback = Closure::wrap(Box::new(move |event: Event| {
        event.prevent_default();
        let form = event.target().unwrap().dyn_into::<HtmlFormElement>().unwrap();
        let name = input_value(&form, "name"); let notes = input_value(&form, "notes");
        let (time_key, time) = if booking_form { ("loanTime", input_value(&form, "loanTime")) } else { ("returnTime", input_value(&form, "returnTime")) };
        let fuel = if !booking_form { format!(r#","fuel":"{}""#, esc(&input_value(&form, "fuel"))) } else { String::new() };
        let json = format!(r#"{{"name":"{}","{}":"{}","notes":"{}"{}{}{}}}"#, esc(&name), time_key, esc(&time), esc(&notes), fuel, if extra == "{}" {String::new()} else {format!(",{}", &extra[1..extra.len()-1])}, "");
        let endpoint = endpoint.clone(); let state = state.clone(); let success = success.to_string();
        spawn_local(async move {
                let mut init = RequestInit::new(); init.set_method("POST"); init.set_mode(RequestMode::SameOrigin); init.set_body(&JsValue::from_str(&json));
            let request = Request::new_with_str_and_init(&endpoint, &init).unwrap(); request.headers().set("Content-Type", "application/json").unwrap();
            match JsFuture::from(web_sys::window().unwrap().fetch_with_request(&request)).await {
                Ok(response) => { let response: Response = response.dyn_into().unwrap(); let body = JsFuture::from(response.json().unwrap()).await.unwrap(); if response.ok() { let result: serde_json::Value = serde_wasm_bindgen::from_value(body).unwrap(); *state.borrow_mut() = serde_json::from_value(result["vehicles"].clone()).unwrap(); close_modal(); render(state); set_toast(&success); } else { set_toast("Terjadi kesalahan."); } },
                Err(_) => set_toast("Server belum tersedia."),
            }
        });
    }) as Box<dyn FnMut(_)>);
    form.add_event_listener_with_callback("submit", callback.as_ref().unchecked_ref()).unwrap(); callback.forget();
}

#[wasm_bindgen(start)]
pub fn start() {
    let date = js_sys::Date::new_0();
    let months = ["Januari", "Februari", "Maret", "April", "Mei", "Juni", "Juli", "Agustus", "September", "Oktober", "November", "Desember"];
    let month = months.get(date.get_month() as usize).copied().unwrap_or("Januari");
    let today = format!("{} {} {}", date.get_date(), month, date.get_full_year());
    el("today").set_text_content(Some(&today));
    let state: Rc<RefCell<Vec<Vehicle>>> = Rc::new(RefCell::new(Vec::new()));
    let fleet_state = state.clone();
    let click = Closure::wrap(Box::new(move |event: Event| {
        let target: Element = event.target().unwrap().dyn_into().unwrap();
        if let Ok(Some(eye)) = target.closest(".view-booking") {
            event.stop_propagation();
            let id = eye.get_attribute("data-id").unwrap();
            if let Some(v) = fleet_state.borrow().iter().find(|v| v.id == id).cloned() { show_booking_detail(v); }
        } else if let Ok(Some(card)) = target.closest(".vehicle") {
            let id = card.get_attribute("data-id").unwrap();
            if let Some(v) = fleet_state.borrow().iter().find(|v| v.id == id).cloned() {
                if v.status == "available" { show_booking(v, fleet_state.clone()); } else { show_return(v, fleet_state.clone()); }
            }
        }
    }) as Box<dyn FnMut(_)>);
    el("fleet").add_event_listener_with_callback("click", click.as_ref().unchecked_ref()).unwrap(); click.forget();
    let backdrop = el("backdrop");
    let backdrop_click = backdrop.clone();
    let on_backdrop_close = Closure::wrap(Box::new(move |event: Event| {
        let target = event.target().unwrap();
        let backdrop_target: web_sys::EventTarget = backdrop_click.clone().unchecked_into();
        if target == backdrop_target {
            close_modal();
        }
    }) as Box<dyn FnMut(Event)>);
    backdrop.add_event_listener_with_callback("click", on_backdrop_close.as_ref().unchecked_ref()).unwrap();
    on_backdrop_close.forget();

    let close = Closure::wrap(Box::new(move |_| close_modal()) as Box<dyn FnMut(Event)>);
    el("closeModal").add_event_listener_with_callback("click", close.as_ref().unchecked_ref()).unwrap(); close.forget();
    let state_load = state.clone();
    spawn_local(async move {
        let response = JsFuture::from(web_sys::window().unwrap().fetch_with_str("/api/vehicles")).await.unwrap();
        let response: Response = response.dyn_into().unwrap();
        let body = JsFuture::from(response.json().unwrap()).await.unwrap();
        let result: FleetResponse = serde_wasm_bindgen::from_value(body).unwrap();
        *state_load.borrow_mut() = result.vehicles; render(state_load);
    });
}
