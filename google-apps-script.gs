function doGet(e) {
  try {
    var payload = JSON.parse(e.parameter.payload);
    var spreadsheet = SpreadsheetApp.openById(payload.spreadsheetId);
    var sheet = spreadsheet.getSheetByName(payload.sheetName);
    if (!sheet) throw new Error("Sheet tidak ditemukan: " + payload.sheetName);
    sheet.appendRow(payload.row);
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
