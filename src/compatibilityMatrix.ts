export type FileCategory = "image" | "document" | "data" | "archive" | "unknown";

export interface BubbleSlot {
  id: string;
  label: string;
  color: string;
  enabled: boolean;
  lockReason?: string;
}

const IMAGE_EXTS = ["png", "jpg", "jpeg", "webp", "bmp", "gif", "ico", "tiff"];
const DOC_EXTS = ["pdf", "docx", "txt", "md", "html"];
const DATA_EXTS = ["csv", "json"];
const ARCHIVE_EXTS = ["zip"];

export function extractExt(filePath: string): string {
  const name = filePath.split(/[\\/]/).pop() || "";
  if (!name.includes(".")) return "";
  const ext = name.split(".").pop()?.toLowerCase() || "";
  return ext === "jpeg" ? "jpg" : ext;
}

export function detectCategory(extRaw: string): FileCategory {
  const ext = extRaw.replace(".", "").toLowerCase();
  const normalized = ext === "jpeg" ? "jpg" : ext;
  if (IMAGE_EXTS.includes(normalized)) return "image";
  if (DOC_EXTS.includes(normalized)) return "document";
  if (DATA_EXTS.includes(normalized)) return "data";
  if (ARCHIVE_EXTS.includes(normalized)) return "archive";
  return "unknown";
}

export function getWheelForFile(extRaw: string): BubbleSlot[] {
  const ext = extRaw.replace(".", "").toLowerCase();
  const normalizedExt = ext === "jpeg" ? "jpg" : ext;
  const category = detectCategory(normalizedExt);

  if (category === "image") {
    const slots = [
      { id: "webp", label: "WebP", color: "border-emerald-400/80 text-emerald-300" },
      { id: "png", label: "PNG", color: "border-sky-400/80 text-sky-300" },
      { id: "jpg", label: "JPG", color: "border-amber-400/80 text-amber-300" },
      { id: "pdf", label: "PDF", color: "border-rose-400/80 text-rose-300" },
      { id: "ico", label: "ICO", color: "border-purple-400/80 text-purple-300" },
      { id: "compress", label: "Slim", color: "border-teal-400/80 text-teal-300" },
      { id: "zip", label: "ZIP", color: "border-indigo-400/80 text-indigo-300" },
      { id: "docx", label: "DOCX", color: "border-blue-400/80 text-blue-300" },
    ];

    return slots.map((s) => {
      if (s.id === normalizedExt) {
        return { ...s, enabled: false, lockReason: `Il file è già in formato .${normalizedExt.toUpperCase()}` };
      }
      if (s.id === "docx") {
        return { ...s, enabled: false, lockReason: "Foto ➔ Word non supportato (usa PDF)" };
      }
      return { ...s, enabled: true };
    });
  }

  if (category === "document") {
    const slots = [
      { id: "pdf", label: "PDF", color: "border-rose-400/80 text-rose-300" },
      { id: "docx", label: "DOCX", color: "border-blue-400/80 text-blue-300" },
      { id: "txt", label: "TXT", color: "border-zinc-300/80 text-zinc-200" },
      { id: "md", label: "MD", color: "border-violet-400/80 text-violet-300" },
      { id: "html", label: "HTML", color: "border-orange-400/80 text-orange-300" },
      { id: "zip", label: "ZIP", color: "border-indigo-400/80 text-indigo-300" },
      { id: "webp", label: "WebP", color: "border-emerald-400/80 text-emerald-300" },
      { id: "png", label: "PNG", color: "border-sky-400/80 text-sky-300" },
    ];

    return slots.map((s) => {
      if (s.id === normalizedExt) {
        return { ...s, enabled: false, lockReason: `Il documento è già .${normalizedExt.toUpperCase()}` };
      }
      if (s.id === "webp" || s.id === "png") {
        return { ...s, enabled: false, lockReason: "Documento testuale ➔ Immagine bloccato" };
      }
      return { ...s, enabled: true };
    });
  }

  if (category === "data") {
    const slots = [
      { id: "json", label: "JSON", color: "border-amber-400/80 text-amber-300" },
      { id: "csv", label: "CSV", color: "border-emerald-400/80 text-emerald-300" },
      { id: "html", label: "HTML", color: "border-orange-400/80 text-orange-300" },
      { id: "md", label: "MD", color: "border-violet-400/80 text-violet-300" },
      { id: "pdf", label: "PDF", color: "border-rose-400/80 text-rose-300" },
      { id: "txt", label: "TXT", color: "border-zinc-300/80 text-zinc-200" },
      { id: "zip", label: "ZIP", color: "border-indigo-400/80 text-indigo-300" },
      { id: "ico", label: "ICO", color: "border-purple-400/80 text-purple-300" },
    ];

    return slots.map((s) => {
      if (s.id === normalizedExt) {
        return { ...s, enabled: false, lockReason: `I dati sono già in .${normalizedExt.toUpperCase()}` };
      }
      if (s.id === "ico") {
        return { ...s, enabled: false, lockReason: "Dati tabellari ➔ Icona non supportato" };
      }
      return { ...s, enabled: true };
    });
  }

  if (category === "archive") {
    return [
      { id: "unzip", label: "Estrai", color: "border-emerald-400/80 text-emerald-300", enabled: true },
      { id: "pdf", label: "PDF", color: "border-rose-400/80 text-rose-300", enabled: false, lockReason: "Estrai prima lo ZIP" },
      { id: "docx", label: "DOCX", color: "border-blue-400/80 text-blue-300", enabled: false, lockReason: "Estrai prima lo ZIP" },
      { id: "webp", label: "WebP", color: "border-emerald-400/80 text-emerald-300", enabled: false, lockReason: "Estrai prima lo ZIP" },
      { id: "png", label: "PNG", color: "border-sky-400/80 text-sky-300", enabled: false, lockReason: "Estrai prima lo ZIP" },
      { id: "jpg", label: "JPG", color: "border-amber-400/80 text-amber-300", enabled: false, lockReason: "Estrai prima lo ZIP" },
      { id: "txt", label: "TXT", color: "border-zinc-300/80 text-zinc-200", enabled: false, lockReason: "Estrai prima lo ZIP" },
      { id: "zip", label: "ZIP", color: "border-indigo-400/80 text-indigo-300", enabled: false, lockReason: "È già un archivio .ZIP" },
    ];
  }

  return [
    { id: "zip", label: "ZIP", color: "border-indigo-400/80 text-indigo-300", enabled: true },
    { id: "pdf", label: "PDF", color: "border-rose-400/80 text-rose-300", enabled: false, lockReason: "Formato non convertibile in PDF" },
    { id: "docx", label: "DOCX", color: "border-blue-400/80 text-blue-300", enabled: false, lockReason: "Formato non convertibile in Word" },
    { id: "webp", label: "WebP", color: "border-emerald-400/80 text-emerald-300", enabled: false, lockReason: "Non è un'immagine" },
    { id: "png", label: "PNG", color: "border-sky-400/80 text-sky-300", enabled: false, lockReason: "Non è un'immagine" },
    { id: "jpg", label: "JPG", color: "border-amber-400/80 text-amber-300", enabled: false, lockReason: "Non è un'immagine" },
    { id: "txt", label: "TXT", color: "border-zinc-300/80 text-zinc-200", enabled: false, lockReason: "Formato binario non testuale" },
    { id: "json", label: "JSON", color: "border-amber-400/80 text-amber-300", enabled: false, lockReason: "Non è un file dati CSV" },
  ];
}

// Calcola la ruota per più file: sblocca SOLO i formati compatibili con TUTTI i file del batch!
export function getWheelForFiles(filePaths: string[]): BubbleSlot[] {
  if (filePaths.length <= 1) {
    return getWheelForFile(extractExt(filePaths[0] || ""));
  }

  const exts = filePaths.map(extractExt);
  const perFileWheels = exts.map((e) => getWheelForFile(e));
  const baseWheel = perFileWheels[0];

  return baseWheel.map((slot) => {
    // In modalità batch, creare un unico ZIP combinato è sempre valido!
    if (slot.id === "zip") {
      return { ...slot, enabled: true, lockReason: undefined };
    }

    const supportedByAll = perFileWheels.every((wheel) => {
      const matching = wheel.find((s) => s.id === slot.id);
      return matching ? matching.enabled : false;
    });

    if (!supportedByAll) {
      return {
        ...slot,
        enabled: false,
        lockReason: `Batch misto: .${slot.label} non è compatibile con tutti i ${filePaths.length} file`,
      };
    }

    return slot;
  });
}