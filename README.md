# 🔮 Morph
**Convertitore offline rapido per Windows, pensato per azioni frequenti e file privati.**

[![Tauri v2](https://img.shields.io/badge/Tauri-v2-24C8DB?style=flat-square&logo=tauri&logoColor=white)](https://v2.tauri.app/)
[![Rust](https://img.shields.io/badge/Rust-Edition_2021-000000?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Windows_10%2F11-0078D6?style=flat-square&logo=windows)](https://www.microsoft.com/windows)
[![License: MIT](https://img.shields.io/badge/License-MIT-emerald.svg?style=flat-square)](LICENSE)

Morph è una utility desktop nativa per Windows che permette di convertire immagini, file dati (CSV/JSON), testo/documenti e archivi ZIP localmente sul proprio PC, senza caricare file privati su servizi web esterni.

## 🚀 Caratteristiche Principali

* **Select & Summon (Alt + M):** Seleziona uno o più file in Esplora File e premi Alt + M per caricarli istantaneamente nella ruota di conversione.
* **Batch Atomico Multi-File:** Supporta il trascinamento multiplo con calcolo automatico dell'intersezione dei formati compatibili e rollback automatico in caso di file corrotti.
* **Fast-Track da Tastiera:** Premi i tasti 1-8 per scegliere il formato o Invio per convertire direttamente nel tuo formato ★ Preferito (impostabile col tasto destro).
* **System Tray & Autostart:** Vive silenziosamente nella System Tray di Windows con avvio automatico opzionale e scelta della cartella di destinazione (*Stessa cartella*, *Desktop*, *Download*).

## 🔄 Formati Supportati e Ambito d'Uso

| Categoria | Input | Output Supportati | Note Tecniche |
| :--- | :--- | :--- | :--- |
| **Immagini** | PNG, JPG, WebP, ICO, BMP, GIF, TIFF | WebP, PNG, JPG, PDF, ICO, Slim, ZIP | Elaborazione nativa lossless/lossy tramite crate image |
| **Dati & Fogli** | CSV, JSON | JSON, CSV, HTML, MD, PDF, TXT, ZIP | Parser conforme RFC-4180 (supporta ,, ;, \t e flattening di JSON annidati) |
| **Documenti** | PDF, DOCX, TXT, MD, HTML | PDF, DOCX, TXT, MD, HTML, ZIP | *Conversione orientata al contenuto testuale* (preserva paragrafi, titoli # ed elenchi; non sostituisce suite DTP per layout complessi) |
| **Archivi** | ZIP / Qualsiasi file | Estrai / ZIP | Supporta anche l'unione di batch multi-file in un singolo Morph_Bundle.zip |

## 🛠️ Sviluppo e Test

```bash
npm install
npm run tauri dev

# Eseguire la suite di test automatici Rust
cd src-tauri && cargo test

# Compilare l'installer NSIS/MSI per Windows (~2.3 MiB)
npm run tauri build
```

## 📄 Licenza
Rilasciato sotto licenza [MIT](LICENSE).
