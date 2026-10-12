# Armada KPP Pratama Ternate

Aplikasi lokal untuk peminjaman dan pengembalian kendaraan dinas.

## Menjalankan

### Dengan Docker (disarankan untuk laptop lain)

Pasang Docker Desktop (Windows/macOS) atau Docker Engine dengan Docker Compose (Linux), lalu dari folder aplikasi jalankan:

```bash
docker compose up --build
```

Buka `http://localhost:3000`. Untuk menghentikan aplikasi, tekan `Ctrl+C`; atau jalankan `docker compose down` dari terminal lain. Data aplikasi disimpan di folder `data` dan tetap ada setelah container dihentikan atau dibangun ulang.

Pindahkan folder source aplikasi ke laptop tujuan. Saat menyalin, jangan sertakan `data/bookings.json` kecuali riwayat transaksi lama memang ingin dipindahkan. Docker tidak memasukkan file itu ke image; aplikasi menyimpan data laptop tujuan di folder `data`.

### Tanpa Docker (Linux/macOS)

Perlu Rust/Cargo terpasang:

```bash
./start.sh
```

Buka `http://localhost:3000`. Data disimpan di `data/bookings.json`. Untuk menghentikan aplikasi:

```bash
./stop.sh
```

Alternatifnya, gunakan `npm start` dan `npm stop`.

Untuk mencatat transaksi ke Google Spreadsheet, salin isi `google-apps-script.gs` ke project Apps Script yang terhubung ke akun Google yang memiliki spreadsheet ini, deploy sebagai **Web app** dengan akses **Anyone**, lalu gunakan URL deployment. Script menggunakan `doGet` agar kompatibel dengan redirect ContentService Apps Script:

```bash
export GOOGLE_APPS_SCRIPT_URL="https://script.google.com/macros/s/URL_DEPLOYMENT/exec"
export GOOGLE_SPREADSHEET_ID="ID_SPREADSHEET_MILIKMU"
export GOOGLE_BOOKING_SHEET="peminjaman"
export GOOGLE_RETURN_SHEET="pengembalian"
```

Jalankan `npm start` setelah mengatur variabel untuk mode tanpa Docker, atau `docker compose up --build` untuk Docker. Jangan masukkan URL deployment, ID spreadsheet, atau kredensial ke source publik. Buat dua tab bernama `peminjaman` dan `pengembalian`. Sheet `peminjaman` menerima waktu transaksi, kendaraan, nama, jam peminjaman, keterangan, dan status. Sheet `pengembalian` menerima waktu transaksi, kendaraan, nama pengembali, jam pengembalian, pengisian bensin, dan keterangan.

Alternatifnya, integrasi Google Sheets API langsung tetap didukung melalui `GOOGLE_SPREADSHEET_ID` dan `GOOGLE_SERVICE_ACCOUNT_JSON`.

Catatan pemasukan dan pengeluaran tersimpan otomatis di `data/bookings.json`. Dasbor menampilkan rekap harian berdasarkan waktu WIT; setiap transaksi dicatat ke hari saat transaksi dibuat dan jenisnya mengikuti klasifikasi otomatis dari catatan.

Jika asisten belum mengenali istilah pada transaksi, asisten akan menanyakan apakah transaksi termasuk pemasukan atau pengeluaran. Setelah dipilih, istilah tersebut disimpan sebagai frasa deteksi dan digunakan otomatis untuk transaksi berikutnya.
