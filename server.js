const http = require("http");
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");
const { URL } = require("url");

const PORT = Number(process.env.PORT || 3000);
const root = path.join(__dirname, "public");
const vehicles = [
  { id: "innova-1321", name: "Innova", plate: "1321", color: "black", icon: "🚗" },
  { id: "xpander-1217", name: "Xpander", plate: "1217", color: "black", icon: "🚘" },
  { id: "strada", name: "Strada", plate: "—", color: "black", icon: "🛻" },
  { id: "terios", name: "Terios", plate: "—", color: "silver", icon: "🚙" }
];
const bookings = [];

function json(res, status, payload) {
  res.writeHead(status, { "Content-Type": "application/json; charset=utf-8" });
  res.end(JSON.stringify(payload));
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    let raw = "";
    req.on("data", chunk => {
      raw += chunk;
      if (raw.length > 1e6) req.destroy();
    });
    req.on("end", () => {
      try { resolve(raw ? JSON.parse(raw) : {}); } catch (error) { reject(error); }
    });
    req.on("error", reject);
  });
}

function currentState() {
  return vehicles.map(vehicle => {
    const active = bookings.find(item => item.vehicleId === vehicle.id && item.status === "booked");
    return { ...vehicle, status: active ? "booked" : "available", booking: active || null };
  });
}

async function googleSheetsAppend(sheetName, row) {
  const appsScriptUrl = process.env.GOOGLE_APPS_SCRIPT_URL;
  if (appsScriptUrl) {
    const spreadsheetId = process.env.GOOGLE_SPREADSHEET_ID;
    if (!spreadsheetId) throw new Error("GOOGLE_SPREADSHEET_ID belum diatur");
    const payload = { spreadsheetId, sheetName, row };
    const query = new URLSearchParams({ payload: JSON.stringify(payload) });
    let response = await fetch(`${appsScriptUrl}?${query}`, { redirect: "follow" });
    if (!response.ok) throw new Error(`Google Apps Script append failed: ${response.status}`);
    const result = await response.json();
    if (!result.ok) throw new Error(result.error || "Google Apps Script menolak data");
    return;
  }
  const spreadsheetId = process.env.GOOGLE_SPREADSHEET_ID;
  const serviceAccount = process.env.GOOGLE_SERVICE_ACCOUNT_JSON;
  if (!spreadsheetId || !serviceAccount) return;
  const account = JSON.parse(serviceAccount);
  const now = Math.floor(Date.now() / 1000);
  const header = Buffer.from(JSON.stringify({ alg: "RS256", typ: "JWT" })).toString("base64url");
  const claim = Buffer.from(JSON.stringify({
    iss: account.client_email,
    scope: "https://www.googleapis.com/auth/spreadsheets",
    aud: "https://oauth2.googleapis.com/token",
    exp: now + 3600,
    iat: now
  })).toString("base64url");
  const signature = crypto.createSign("RSA-SHA256");
  signature.update(`${header}.${claim}`);
  const jwt = `${header}.${claim}.${signature.sign(account.private_key, "base64url")}`;
  const tokenResponse = await fetch("https://oauth2.googleapis.com/token", {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({ grant_type: "urn:ietf:params:oauth:grant-type:jwt-bearer", assertion: jwt })
  });
  if (!tokenResponse.ok) throw new Error(`Google auth failed: ${tokenResponse.status}`);
  const { access_token: token } = await tokenResponse.json();
  const range = encodeURIComponent(`${sheetName}!A:Z`);
  const response = await fetch(`https://sheets.googleapis.com/v4/spreadsheets/${spreadsheetId}/values/${range}:append?valueInputOption=USER_ENTERED&insertDataOption=INSERT_ROWS`, {
    method: "POST",
    headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
    body: JSON.stringify({ values: [row] })
  });
  if (!response.ok) throw new Error(`Google Sheets append failed: ${response.status}`);
}

async function handleApi(req, res, url) {
  if (req.method === "GET" && url.pathname === "/api/vehicles") return json(res, 200, { vehicles: currentState() });
  if (req.method !== "POST") return json(res, 405, { error: "Method not allowed" });
  let body;
  try { body = await readBody(req); } catch { return json(res, 400, { error: "Payload tidak valid" }); }

  if (url.pathname === "/api/bookings") {
    const vehicle = vehicles.find(item => item.id === body.vehicleId);
    if (!vehicle || !body.name || !body.loanTime) return json(res, 400, { error: "Nama dan waktu peminjaman wajib diisi." });
    if (bookings.some(item => item.vehicleId === vehicle.id && item.status === "booked")) return json(res, 409, { error: "Mobil sedang dipinjam." });
    const booking = {
      id: crypto.randomUUID(), vehicleId: vehicle.id, vehicle: `${vehicle.name} ${vehicle.plate}`.trim(),
      name: String(body.name).trim(), loanTime: body.loanTime, notes: String(body.notes || "").trim(),
      status: "booked", createdAt: new Date().toISOString()
    };
    bookings.push(booking);
    googleSheetsAppend(process.env.GOOGLE_BOOKING_SHEET || "peminjaman", [booking.createdAt, booking.vehicle, booking.name, booking.loanTime, booking.notes, "Dipinjam"])
      .catch(error => console.error("Gagal menyimpan peminjaman ke spreadsheet:", error.message));
    return json(res, 201, { booking, vehicles: currentState() });
  }

  if (url.pathname.endsWith("/return")) {
    const id = url.pathname.split("/")[3];
    const booking = bookings.find(item => item.id === id && item.status === "booked");
    if (!booking) return json(res, 404, { error: "Peminjaman tidak ditemukan." });
    if (!body.name || !body.returnTime || !body.fuel) return json(res, 400, { error: "Nama, waktu pengembalian, dan pengisian bensin wajib diisi." });
    booking.returnName = String(body.name).trim();
    booking.returnTime = body.returnTime;
    booking.fuel = String(body.fuel).trim();
    booking.returnNotes = String(body.notes || "").trim();
    booking.status = "returned";
    googleSheetsAppend(process.env.GOOGLE_RETURN_SHEET || "pengembalian", [new Date().toISOString(), booking.vehicle, booking.returnName, booking.returnTime, booking.fuel, booking.returnNotes])
      .catch(error => console.error("Gagal menyimpan pengembalian ke spreadsheet:", error.message));
    return json(res, 200, { booking, vehicles: currentState() });
  }
  return json(res, 404, { error: "Endpoint tidak ditemukan" });
}

function serveStatic(req, res) {
  const requested = new URL(req.url, `http://${req.headers.host}`).pathname;
  const file = path.normalize(path.join(root, requested === "/" ? "index.html" : requested));
  if (!file.startsWith(root)) return json(res, 403, { error: "Forbidden" });
  fs.readFile(file, (error, content) => {
    if (error) return json(res, 404, { error: "Not found" });
    const type = { ".html": "text/html", ".css": "text/css", ".js": "text/javascript", ".svg": "image/svg+xml" }[path.extname(file)] || "application/octet-stream";
    res.writeHead(200, { "Content-Type": `${type}; charset=utf-8` });
    res.end(content);
  });
}

http.createServer((req, res) => {
  const url = new URL(req.url, `http://${req.headers.host}`);
  if (url.pathname.startsWith("/api/")) return handleApi(req, res, url).catch(error => json(res, 500, { error: error.message }));
  serveStatic(req, res);
}).listen(PORT, () => console.log(`KPP Fleet app running at http://localhost:${PORT}`));
