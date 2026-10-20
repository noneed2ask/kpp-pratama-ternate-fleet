var SHEET_HEADERS = {
  peminjaman: ["Waktu dicatat", "Kendaraan", "Nama peminjam", "Waktu peminjaman", "Nomor ST", "Tujuan / keterangan", "Jam Check-in", "Jam Check-out", "Status", "Tipe peminjaman"],
  pengembalian: ["Waktu dicatat", "Kendaraan", "Nama pengembali", "Waktu pengembalian", "Pengisian bensin", "Keterangan"]
};

function setupFleetSpreadsheet() {
  var spreadsheet = SpreadsheetApp.create("Peminjaman Kendaraan Dinas - KPP Pratama Ternate");
  var bookingSheet = spreadsheet.getSheets()[0];
  bookingSheet.setName("peminjaman");
  bookingSheet.appendRow(SHEET_HEADERS.peminjaman);
  spreadsheet.insertSheet("pengembalian").appendRow(SHEET_HEADERS.pengembalian);
  Logger.log("GOOGLE_SPREADSHEET_ID=" + spreadsheet.getId());
  Logger.log("SPREADSHEET_URL=" + spreadsheet.getUrl());
}

function ensureBookingCheckTimeColumns(sheet) {
  var headers = sheet.getRange(1, 1, 1, sheet.getLastColumn()).getDisplayValues()[0];
  var destinationColumn = headers.indexOf("Tujuan / keterangan") + 1;
  if (!destinationColumn) throw new Error("Kolom Tujuan / keterangan tidak ditemukan");
  var previousColumn = destinationColumn;
  ["Jam Check-in", "Jam Check-out"].forEach(function(header) {
    var column = headers.indexOf(header) + 1;
    if (!column) {
      sheet.insertColumnAfter(previousColumn);
      column = previousColumn + 1;
      sheet.getRange(1, column).setValue(header);
      headers.splice(column - 1, 0, header);
    }
    previousColumn = column;
  });
  headers = sheet.getRange(1, 1, 1, sheet.getLastColumn()).getDisplayValues()[0];
  var lastRow = sheet.getLastRow();
  var timeZone = sheet.getParent().getSpreadsheetTimeZone();
  ["Waktu peminjaman", "Jam Check-in", "Jam Check-out", "Waktu pengembalian"].forEach(function(header) {
    var column = headers.indexOf(header) + 1;
    if (!column || lastRow < 2) return;
    var range = sheet.getRange(2, column, lastRow - 1, 1);
    var values = range.getValues();
    var changed = false;
    values = values.map(function(row) {
      var value = row[0];
      if (!value) return row;
      var date = value instanceof Date ? value : new Date(value);
      if (isNaN(date.getTime())) return row;
      changed = true;
      return [formatDateTimeText(date, timeZone)];
    });
    if (changed) range.setValues(values);
  });
}

function formatDateTimeText(value, timeZone) {
  var date = value instanceof Date ? value : new Date(value);
  if (isNaN(date.getTime())) throw new Error("Waktu tanggal tidak valid");
  return Utilities.formatDate(date, timeZone, "dd/MM/yyyy HH:mm");
}

function doGet(e) {
  try {
    if (!e || !e.parameter || !e.parameter.payload) throw new Error("Payload tidak ditemukan");
    var payload = JSON.parse(e.parameter.payload);
    var spreadsheet = SpreadsheetApp.openById(payload.spreadsheetId);
    if (payload.action === "employeeNames") {
      var employeeSheet = spreadsheet.getSheetByName("pegawai");
      if (!employeeSheet) throw new Error("Sheet pegawai tidak ditemukan");
      var lastRow = employeeSheet.getLastRow();
      var lastColumn = employeeSheet.getLastColumn();
      if (!lastRow || !lastColumn) throw new Error("Sheet pegawai belum berisi nama");
      var employeeHeaders = employeeSheet.getRange(1, 1, 1, lastColumn).getDisplayValues()[0];
      var nameColumn = employeeHeaders.findIndex(function(header) {
        return header.trim().toLowerCase().replace(/[^a-z0-9]/g, "").indexOf("nama") === 0;
      });
      if (nameColumn < 0) throw new Error("Kolom nama tidak ditemukan pada sheet pegawai");
      var employees = lastRow > 1
        ? employeeSheet.getRange(2, nameColumn + 1, lastRow - 1, 1).getDisplayValues()
            .map(function(row) { return row[0].trim(); })
            .filter(function(name, index, names) { return name && names.indexOf(name) === index; })
        : [];
      return output({ ok: true, employees: employees });
    }
    if (!Object.prototype.hasOwnProperty.call(SHEET_HEADERS, payload.sheetName)) {
      throw new Error("Sheet tidak diizinkan: " + payload.sheetName);
    }
    var sheet = spreadsheet.getSheetByName(payload.sheetName);
    if (!sheet) throw new Error("Sheet tidak ditemukan: " + payload.sheetName);
    if (payload.sheetName === "peminjaman") ensureBookingCheckTimeColumns(sheet);
    if (payload.action === "bookingCounts") {
      var counts = {};
      var lastRow = sheet.getLastRow();
      if (lastRow > 1) {
        sheet.getRange(2, 2, lastRow - 1, 1).getDisplayValues().forEach(function(row) {
          var vehicle = row[0].trim();
          if (vehicle) counts[vehicle] = (counts[vehicle] || 0) + 1;
        });
      }
      return output({ ok: true, counts: counts });
    }
    if (payload.action === "updateBookingCheckTimes") {
      if (payload.sheetName !== "peminjaman") throw new Error("Pembaruan waktu hanya tersedia di tab peminjaman");
      var sheetHeaders = sheet.getRange(1, 1, 1, sheet.getLastColumn()).getDisplayValues()[0];
      var checkInColumn = sheetHeaders.indexOf("Jam Check-in") + 1;
      var checkOutColumn = sheetHeaders.indexOf("Jam Check-out") + 1;
      if (!checkInColumn || !checkOutColumn) throw new Error("Kolom Jam Check-in dan Jam Check-out belum tersedia");
      var bookingTimestamp = new Date(payload.bookingCreatedAt).getTime();
      if (!Number.isFinite(bookingTimestamp)) throw new Error("Waktu pencatatan peminjaman tidak valid");
      var lastRow = sheet.getLastRow();
      var matchingRow = 0;
      if (lastRow > 1) {
        var timestamps = sheet.getRange(2, 1, lastRow - 1, 1).getValues();
        for (var index = 0; index < timestamps.length; index++) {
          var timestamp = timestamps[index][0];
          var timestampValue = timestamp instanceof Date ? timestamp.getTime() : new Date(timestamp).getTime();
          if (timestampValue === bookingTimestamp) {
            matchingRow = index + 2;
            break;
          }
        }
      }
      if (!matchingRow) throw new Error("Peminjaman tidak ditemukan di spreadsheet");
      var updated = false;
      if (Object.prototype.hasOwnProperty.call(payload, "checkInTime")) {
        sheet.getRange(matchingRow, checkInColumn).setValue(
          formatDateTimeText(payload.checkInTime, spreadsheet.getSpreadsheetTimeZone())
        );
        updated = true;
      }
      if (Object.prototype.hasOwnProperty.call(payload, "checkOutTime")) {
        sheet.getRange(matchingRow, checkOutColumn).setValue(
          formatDateTimeText(payload.checkOutTime, spreadsheet.getSpreadsheetTimeZone())
        );
        updated = true;
      }
      if (!updated) throw new Error("Waktu Check-in atau Check-out tidak ditemukan");
      return output({ ok: true });
    }
    var headers = SHEET_HEADERS[payload.sheetName];
    if (!Array.isArray(payload.row) || payload.row.length !== headers.length) throw new Error("Jumlah kolom tidak sesuai");
    var row = payload.row.slice();
    var timeZone = spreadsheet.getSpreadsheetTimeZone();
    headers.forEach(function(header, index) {
      if (["Waktu peminjaman", "Jam Check-in", "Jam Check-out", "Waktu pengembalian"].indexOf(header) !== -1 &&
          typeof row[index] === "string" && row[index]) {
        row[index] = formatDateTimeText(row[index], timeZone);
      }
    });
    sheet.appendRow(row);
    return output({ ok: true });
  } catch (error) {
    return output({ ok: false, error: error.message });
  }
}

function output(data) {
  return ContentService
    .createTextOutput(JSON.stringify(data))
    .setMimeType(ContentService.MimeType.JSON);
}
