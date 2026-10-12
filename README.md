# Peminjaman Kendaraan Dinas

Aplikasi lokal peminjaman dan pengembalian kendaraan dinas KPP Pratama Ternate.

## Menjalankan tanpa Docker

Pasang Git dan Rust/Cargo. Clone repository lalu jalankan backend dari folder project:

```bash
git clone https://github.com/noneed2ask/kpp-pratama-ternate-fleet.git
cd kpp-pratama-ternate-fleet
cargo run --release
```

Build pertama mengunduh dependency Rust dan mungkin perlu beberapa menit. Setelah server aktif, buka <http://localhost:3000>. Hentikan server dengan `Ctrl+C`. Di Windows, jalankan perintah yang sama melalui PowerShell atau Windows Terminal setelah memasang Rust dengan `rustup`.

Di Linux/macOS, script `./start.sh` dan `./stop.sh` juga tersedia.

## Menjalankan dengan Docker

Pasang Docker Desktop (Windows/macOS) atau Docker Engine dan Docker Compose (Linux), lalu jalankan:

```bash
docker compose up --build
```

Buka <http://localhost:3000>. Hentikan dengan `Ctrl+C` atau `docker compose down`. Data aplikasi disimpan di `data/bookings.json` dan tidak disertakan dalam repository.

## Google Spreadsheet (opsional)

Secara opsional, aplikasi dapat mencatat peminjaman dan pengembalian ke Google Spreadsheet. Deploy `google-apps-script.gs` sebagai Web app, siapkan tab `peminjaman` dan `pengembalian`, lalu atur variabel `GOOGLE_APPS_SCRIPT_URL` dan `GOOGLE_SPREADSHEET_ID` di environment lokal sebelum menjalankan aplikasi. Jangan commit URL deployment atau ID spreadsheet ke repository publik.
