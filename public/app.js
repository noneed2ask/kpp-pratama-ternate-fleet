const form = document.querySelector('#entryForm');
const summaryIncomeEl = document.querySelector('#incomeTotal');
const summaryExpenseEl = document.querySelector('#expenseTotal');
const summaryBalanceEl = document.querySelector('#balanceTotal');
const categoryBreakdownEl = document.querySelector('#categoryBreakdown');
const monthLabelEl = document.querySelector('#monthLabel');
const toastEl = document.querySelector('#toast');
const chatInput = document.querySelector('#chatInput');
const botPanel = document.querySelector('#botPanel');
const botTrigger = document.querySelector('#botTrigger');
const chatThread = document.querySelector('#chatThread');
const dailyDate = document.querySelector('#dailyDate');
const dailyIncomeEl = document.querySelector('#dailyIncome');
const dailyExpenseEl = document.querySelector('#dailyExpense');
const dailyBalanceEl = document.querySelector('#dailyBalance');
const dailyEntriesEl = document.querySelector('#dailyEntries');
const dailyDateLabel = document.querySelector('#dailyDateLabel');
let pendingChatMessage = null;

const currency = new Intl.NumberFormat('id-ID', { style: 'currency', currency: 'IDR', maximumFractionDigits: 0 });
const monthFormatter = new Intl.DateTimeFormat('id-ID', { month: 'long', year: 'numeric' });

function notify(message) {
  toastEl.textContent = message;
  toastEl.classList.add('show');
  window.clearTimeout(notify.timeout);
  notify.timeout = window.setTimeout(() => toastEl.classList.remove('show'), 2600);
}

function addChatMessage(role, text) {
  const message = document.createElement('div');
  message.className = `message ${role}`;
  const bubble = document.createElement('div');
  bubble.className = 'bubble';
  bubble.innerHTML = text;
  message.appendChild(bubble);
  chatThread.appendChild(message);
  chatThread.scrollTop = chatThread.scrollHeight;
}

function entryIsIncome(entry) {
  return (entry.type || entry.kind) === 'income';
}

function buildBotReply(entry, summary, reply) {
  const isIncome = entryIsIncome(entry);
  const label = isIncome ? 'Pemasukan' : 'Pengeluaran';
  const signal = isIncome ? '✅' : '📉';
  const amount = formatMoney(entry.amount);
  return `${signal} ${escapeHtml(reply)}<br><strong>${label}: ${amount}</strong> untuk ${escapeHtml(entry.note)}.<br>Saldo sekarang: <strong>${formatMoney(summary.balance)}</strong>.`;
}

function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, (char) => ({
    '&': '&amp;',
    '<': '&lt;',
    '>': '&gt;',
    '"': '&quot;',
    "'": '&#039;'
  }[char]));
}

function formatMoney(value) {
  return currency.format(Number(value || 0));
}

function renderSummary(summary) {
  const income = Number(summary.income || 0);
  const expense = Number(summary.expense || 0);
  const balance = Number(summary.balance || 0);

  summaryIncomeEl.textContent = formatMoney(income);
  summaryExpenseEl.textContent = formatMoney(expense);
  summaryBalanceEl.textContent = formatMoney(balance);

  const incomeCategories = summary.byCategory?.income || {};
  const expenseCategories = summary.byCategory?.expense || {};
  const categoryMap = [{ label: 'Pemasukan', items: incomeCategories, type: 'income' }, { label: 'Pengeluaran', items: expenseCategories, type: 'expense' }];
  const maxValue = Math.max(...Object.values({ ...incomeCategories, ...expenseCategories }).map(Number), 1);

  categoryBreakdownEl.innerHTML = categoryMap.map(({ label, items, type }) => {
    const entries = Object.entries(items);
    if (!entries.length) {
      return `<div class="breakdown-row">
        <div class="head"><span>${label}</span><span>Rp 0</span></div>
        <div class="bar ${type}"><i style="width: 0%"></i></div>
      </div>`;
    }

    return entries.map(([name, value]) => `
      <div class="breakdown-row">
        <div class="head"><span>${escapeHtml(name)}</span><span>${formatMoney(value)}</span></div>
        <div class="bar ${type}"><i style="width: ${(Number(value) / maxValue) * 100}%"></i></div>
      </div>
    `).join('');
  }).join('');

  const now = new Date();
  monthLabelEl.textContent = monthFormatter.format(now);
}

async function loadSummary() {
  const res = await fetch('/api/finance/summary');
  if (!res.ok) throw new Error('Gagal memuat ringkasan');
  const summary = await res.json();
  renderSummary(summary);
}

function localDateInputValue(date) {
  const localDate = new Date(date.getTime() - date.getTimezoneOffset() * 60_000);
  return localDate.toISOString().slice(0, 10);
}

function formatDayLabel(date) {
  const day = new Date(`${date}T12:00:00+09:00`);
  return new Intl.DateTimeFormat('id-ID', {
    weekday: 'long',
    day: 'numeric',
    month: 'long',
    year: 'numeric',
    timeZone: 'Asia/Jayapura'
  }).format(day);
}

function renderDaily(data) {
  dailyDateLabel.textContent = formatDayLabel(data.date);
  dailyIncomeEl.textContent = formatMoney(data.income);
  dailyExpenseEl.textContent = formatMoney(data.expense);
  dailyBalanceEl.textContent = formatMoney(data.balance);
  const entries = data.entries || [];
  if (!entries.length) {
    dailyEntriesEl.innerHTML = '<div class="empty-state">Belum ada transaksi pada tanggal ini.</div>';
    return;
  }
  dailyEntriesEl.innerHTML = entries.map((entry) => `
    <article class="entry-row">
      <div>
        <h3>${escapeHtml(entry.note)}</h3>
        <p>${escapeHtml(entry.category)}</p>
      </div>
      <div class="entry-meta">
        <strong class="${entryIsIncome(entry) ? 'amount-income' : 'amount-expense'}">${entryIsIncome(entry) ? '+' : '-'}${formatMoney(entry.amount)}</strong>
        <span class="pill ${entryIsIncome(entry) ? 'income' : 'expense'}">${entryIsIncome(entry) ? 'Pemasukan' : 'Pengeluaran'}</span>
      </div>
    </article>
  `).join('');
}

function syncDailyDateToToday() {
  const today = localDateInputValue(new Date());
  if (dailyDate.value !== today) {
    dailyDate.value = today;
    loadDaily().catch(() => notify('Rekap harian belum berhasil dimuat.'));
  }
}

async function loadDaily() {
  const response = await fetch(`/api/finance/daily?date=${encodeURIComponent(dailyDate.value)}`);
  if (!response.ok) throw new Error('Gagal memuat rekap harian');
  renderDaily(await response.json());
}

async function refresh() {
  await Promise.all([loadSummary(), loadDaily()]);
}

async function sendChatMessage(message, kind = null) {
  const sendButton = form.querySelector('button[type="submit"]');
  sendButton.disabled = true;

  try {
    const response = await fetch('/api/finance/chat', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ message, ...(kind ? { type: kind } : {}) })
    });
    const result = await response.json().catch(() => ({}));
    if (!response.ok) {
      addChatMessage('bot', escapeHtml(result.error || 'Pesan belum bisa diproses. Coba lagi ya.'));
      return false;
    }

    if (!result.entry) {
      if (result.needsClassification) {
        pendingChatMessage = message;
        chatThread.querySelectorAll('.classification-choices').forEach((choices) => choices.remove());
        addChatMessage('bot', `${escapeHtml(result.reply)}<br><div class="classification-choices"><button type="button" data-finance-kind="income">Pemasukan</button><button type="button" data-finance-kind="expense">Pengeluaran</button></div>`);
      } else {
      addChatMessage('bot', escapeHtml(result.reply || 'Coba tulis transaksi beserta nominalnya.'));
      }
      return false;
    }

    pendingChatMessage = null;
    chatThread.querySelectorAll('.classification-choices').forEach((choices) => choices.remove());
    addChatMessage('bot', buildBotReply(result.entry, result.summary, result.reply));
    refresh().catch(() => notify('Transaksi tersimpan, tetapi ringkasan belum berhasil diperbarui.'));
    return true;
  } catch {
    addChatMessage('bot', 'Koneksi ke server terputus. Pesan belum dicatat, coba kirim lagi ya.');
    return false;
  } finally {
    sendButton.disabled = false;
    chatInput.focus();
  }
}

form.addEventListener('submit', async (event) => {
  event.preventDefault();
  const message = chatInput.value.trim();
  if (!message) return;

  const responseKind = /^(pemasukan|income|masuk)$/i.test(message)
    ? 'income'
    : /^(pengeluaran|expense|keluar)$/i.test(message) ? 'expense' : null;
  if (pendingChatMessage && responseKind) {
    addChatMessage('user', escapeHtml(message));
    chatInput.value = '';
    await sendChatMessage(pendingChatMessage, responseKind);
    return;
  }

  pendingChatMessage = null;
  chatThread.querySelectorAll('.classification-choices').forEach((choices) => choices.remove());
  addChatMessage('user', escapeHtml(message));
  chatInput.value = '';
  await sendChatMessage(message);
});

chatThread.addEventListener('click', async (event) => {
  const button = event.target.closest('[data-finance-kind]');
  if (!button || !pendingChatMessage) return;
  const kind = button.dataset.financeKind;
  addChatMessage('user', kind === 'income' ? 'Pemasukan' : 'Pengeluaran');
  await sendChatMessage(pendingChatMessage, kind);
});

function toggleBotForm(forceOpen) {
  const shouldOpen = typeof forceOpen === 'boolean' ? forceOpen : !botPanel.classList.contains('is-open');
  botPanel.classList.toggle('is-open', shouldOpen);
  botPanel.setAttribute('aria-hidden', String(!shouldOpen));
  if (shouldOpen) {
    chatInput.focus();
  }
}

document.querySelector('#exportCsv').addEventListener('click', () => {
  window.location.href = '/api/finance/export.csv';
});

syncDailyDateToToday();
window.setInterval(() => {
  syncDailyDateToToday();
}, 60_000);
document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'visible') {
    syncDailyDateToToday();
  }
});
dailyDate.addEventListener('change', () => {
  loadDaily().catch(() => notify('Rekap harian belum berhasil dimuat.'));
});

document.querySelector('#resetEntries').addEventListener('click', async () => {
  const confirmed = window.confirm('Yakin ingin menghapus semua transaksi keuangan, termasuk yang tampil di rekap harian? Tindakan ini tidak bisa dibatalkan. Data kendaraan tidak akan terpengaruh.');
  if (!confirmed) {
    return;
  }

  const button = document.querySelector('#resetEntries');
  button.disabled = true;
  try {
    const response = await fetch('/api/finance/entries', { method: 'DELETE' });
    const result = await response.json().catch(() => ({}));
    if (!response.ok) {
      notify(result.error || 'Gagal mereset transaksi.');
      return;
    }

    renderSummary(result.summary);
    renderDaily({ income: 0, expense: 0, balance: 0, entries: [] });
    chatThread.innerHTML = '';
    pendingChatMessage = null;
    addChatMessage('bot', 'Semua transaksi dan rekap harian sudah direset. Silakan mulai catat lagi kapan saja.');
    notify(`${result.removed} transaksi dan rekap hariannya berhasil direset.`);
  } catch {
    notify('Server tidak tersedia. Transaksi belum berhasil direset.');
  } finally {
    button.disabled = false;
  }
});

botTrigger.addEventListener('click', () => toggleBotForm());

document.addEventListener('click', (event) => {
  const clickedInsidePanel = botPanel.contains(event.target);
  const clickedTrigger = event.target === botTrigger || botTrigger.contains(event.target);
  if (!clickedInsidePanel && !clickedTrigger && botPanel.classList.contains('is-open')) {
    toggleBotForm(false);
  }
});

renderSummary({ income: 0, expense: 0, balance: 0, byCategory: { income: {}, expense: {} } });
addChatMessage('bot', 'Halo! Saya siap bantu catat transaksi. Ketik seperti <strong>gaji 2,5jt</strong> atau <strong>makan siang 50rb</strong>.');
refresh().catch(() => notify('Server belum tersedia atau data belum siap.'));
