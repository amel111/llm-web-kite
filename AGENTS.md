# Project instructions — Temper Web UI (proxy.py)

## Verifikasi WAJIB sebelum commit
- **JS**: jalankan `node --check static/app.js` setiap kali `static/app.js` berubah. Jangan commit bila syntax gagal.
- **UI**: setiap perubahan tampilan (template/CSS/JS) WAJIB diverifikasi di browser (screenshot + cek elemen/konsol) SEBELUM commit. Jangan klaim selesai hanya dari perubahan kode.
- **Backend**: uji endpoint yang berubah via HTTP (contoh: `/health`, `/api/models`) sebelum commit.
- Setelah verifikasi UI di browser, **bersihkan data test** yang masuk ke `usage.db` (tabel `messages`/`conversations`) sebelum commit. Jangan tinggalkan pesan test di data user.

## Restart server (wajib setelah edit template)
Flask non-debug MENG-CACHE template. Setelah mengedit apa pun di `templates/`, restart:
```powershell
$srv = (Get-NetTCPConnection -LocalPort 8080 -State Listen -ErrorAction SilentlyContinue).OwningProcess | Select-Object -First 1; if ($srv) { Stop-Process -Id $srv -Force }
```
lalu jalankan ulang `python proxy.py` (background). Perubahan `static/` tidak butuh restart, tapi browser bisa cache — bump parameter `?v=` di URL bila perlu.

## Aturan UI (ChatGPT-style, disetujui user)
- Referensi visual: chatgpt.com. Spec resmi: `docs/superpowers/specs/2026-11-04-chatgpt-style-ui-design.md`.
- Ikon: **inline stroke SVG saja**. Dilarang emoji, icon font, dan web font (font: system-ui stack).
- Tema: manual toggle, light default, disimpan di localStorage via `[data-theme]` pada `<html>`.
- Scrollbar tipis transparan ala ChatGPT: Chromium via `::-webkit-scrollbar`; properti standar `scrollbar-width/color` HANYA di dalam `@supports not selector(::-webkit-scrollbar)` (properti standar menimpa pseudo webkit di Chromium).
- User bubble: `width: fit-content` (bukan max-width persentase ganda — persentase ganda dalam flex shrink-to-fit bikin width salah resolve).
- Jangan menambahkan tombol/elemen UI baru tanpa diminta user.

## Arsitektur singkat
- `proxy.py`: backend Flask (chat, streaming, regenerate/edit/rename, settings, stats, kompatibel OpenAI `/v1/chat/completions`).
- Data chat & stats: `usage.db` (SQLite) lokal. Ollama stateless, tanpa history.
- `static/app.js`: seluruh logic client. `static/ui.css`: seluruh styling (pengganti m3.css yang sudah dihapus).
- Ollama `127.0.0.1:11434`, model aktif: `temper-2-ion:latest` (default, dari `settings.json`) dan `temper-2-axon-flash:latest`. Model picker web otomatis list model dari `/api/models` (live dari Ollama), tanpa hardcode. Model lama dibuat dari `Modelfile`.

## Lingkungan Windows / PowerShell
- `&&` tidak valid (pakai `;`), `grep`/`sed` tidak ada (pakai `Select-String`/`Get-Content`), heredoc diblokir (tulis file script sementara).
- Untuk cek script Python sementara: tulis ke `C:\Users\azz\AppData\Local\Temp\opencode\` lalu jalankan.
- Browser evaluate harus IIFE: `(function(){ ... })()`.

## Gaya commit
Satu baris, ringkas, bahasa Inggris, tanpa body (contoh: `Chat: remove composer attach button`). Jangan push tanpa diminta.
