# Peminjaman Kendaraan Dinas

Aplikasi lokal peminjaman dan pengembalian kendaraan dinas KPP Pratama Ternate.

## Menjalankan tanpa Docker

Pasang Git dan Rust/Cargo. Clone repository, siapkan file login lokal, lalu jalankan backend dari folder project:

```bash
git clone https://github.com/noneed2ask/kpp-pratama-ternate-fleet.git
cd kpp-pratama-ternate-fleet
cp .fleet-auth.env.example .fleet-auth.env
# Isi username dan password di .fleet-auth.env
./start.sh
```

Build pertama mengunduh dependency Rust dan mungkin perlu beberapa menit. Setelah server aktif, buka <http://localhost:3000>. Hentikan server dengan `./stop.sh`. Di Windows, siapkan variabel login dari `.fleet-auth.env.example`, lalu jalankan `cargo run --release` melalui PowerShell atau Windows Terminal setelah memasang Rust dengan `rustup`.

Di Linux/macOS, `./start.sh` membaca konfigurasi dari `.env` dan `.fleet-auth.env` jika tersedia.

Formulir peminjaman dan pengembalian tersedia langsung di aplikasi. Peminjaman meminta **Nomor ST** dan pilihan **Full Day (9,5 jam)** atau **Half Day (5 jam)**. Check-in dan Check-out dicatat di aplikasi; dashboard menampilkan frekuensi peminjaman dan peringatan jika durasi melewati batas pilihan.

Login admin/user dikonfigurasi pada `.fleet-auth.env` (salin `.fleet-auth.env.example`, lalu isi username dan password). User mengajukan pengembalian melalui tombol Check-out; kendaraan tetap dipinjam sampai admin memvalidasi. Hanya setelah validasi, catatan pengembalian dikirim ke spreadsheet dan kendaraan kembali tersedia.
## Menjalankan dengan Docker

Pasang Docker Desktop (Windows/macOS) atau Docker Engine dan Docker Compose (Linux). Siapkan `.fleet-auth.env` seperti di atas, lalu jalankan:

```bash
docker compose up --build
```

Buka <http://localhost:3000>. Hentikan dengan `Ctrl+C` atau `docker compose down`. Data aplikasi disimpan di `data/bookings.json` dan tidak disertakan dalam repository.

## Deploy ke internet dengan Render

Repository menyertakan `render.yaml` untuk membuat Docker web service di Render dengan disk persisten terpasang pada `/app/data`. Disk persisten memerlukan paket berbayar. Gunakan satu instance agar data dan sesi in-memory tetap konsisten.

Untuk deploy, push perubahan aplikasi dan `render.yaml` ke branch GitHub yang akan dipakai, lalu buat Blueprint baru dari repository tersebut di Render. Saat diminta, isi environment variables yang ditandai sebagai secret: URL Apps Script, ID spreadsheet, dan username/password admin serta user. Jangan masukkan nilai rahasia ke `render.yaml` atau Git.

Setelah deploy, akses URL HTTPS yang diberikan Render. Restart atau deploy ulang akan mengharuskan pengguna login lagi; sesi berakhir setelah 30 menit tidak aktif.

## Google Spreadsheet (opsional)

Untuk menyinkronkan peminjaman, pengembalian, check-in/check-out, dan frekuensi dashboard, deploy `google-apps-script.gs` sebagai Web app. Siapkan tab `peminjaman` dan `pengembalian` sesuai header yang dibuat oleh `setupFleetSpreadsheet()`. Salin `.env.example` menjadi `.env`, isi URL deployment dan ID spreadsheet yang sudah dipakai sebelumnya, lalu jalankan aplikasi. Jangan jalankan `setupFleetSpreadsheet()` jika ingin tetap memakai spreadsheet lama.

Tab `peminjaman` memakai kolom **Nomor ST**, **Jam Check-in**, **Jam Check-out**, **Status**, dan **Tipe peminjaman**. Jika dua kolom waktu belum ada, Apps Script akan menambahkannya setelah **Tujuan / keterangan**. Setelah mengubah Apps Script, deploy versi Web app terbaru. Tanpa konfigurasi Google, data armada tetap tersimpan di `data/bookings.json`.

Nama peminjam dan pengembali dimuat dari kolom dengan judul berawalan **Nama** pada baris pertama tab **pegawai**. Setelah memperbarui Apps Script, deploy versi Web app terbaru agar endpoint daftar pegawai tersedia.

Reset data pada halaman admin juga mengosongkan isi tab `peminjaman` dan `pengembalian` tanpa menghapus baris header. Deploy versi Web app terbaru setelah memperbarui Apps Script agar reset tersedia.
