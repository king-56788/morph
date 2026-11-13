import { useState, useEffect, useRef } from "react";
import { motion, AnimatePresence } from "motion/react";
import {
  Sparkles,
  GripHorizontal,
  Check,
  RotateCcw,
  X,
  FileImage,
  FileText,
  FileSpreadsheet,
  FolderArchive,
  FolderOpen,
  AlertCircle,
  Copy,
  Lock,
  Settings,
  EyeOff,
  Keyboard,
  Layers,
  Star,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  getWheelForFiles,
  detectCategory,
  type BubbleSlot,
} from "./compatibilityMatrix";
import "./App.css";

type MorphState =
  | "idle"
  | "radial"
  | "morphing"
  | "done"
  | "error"
  | "settings";

interface MorphResult {
  outputPath: string;
  outputPaths: string[];
  oldSize: number;
  newSize: number;
  filesCount: number;
}

const SHORTCUT_OPTIONS = [
  { id: 0, label: "Alt + M" },
  { id: 1, label: "Alt + Spazio" },
  { id: 2, label: "Ctrl + Spazio" },
  { id: 3, label: "Alt + X" },
];

const OUTPUT_MODES = [
  { id: "same", label: "Stessa" },
  { id: "desktop", label: "Desktop" },
  { id: "downloads", label: "Download" },
];

const RADIUS = 104;
const CENTER = 175;

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb.toFixed(0)} KB`;
  return `${(kb / 1024).toFixed(1)} MB`;
}

function useAnimatedNumber(from: number, to: number, duration = 650) {
  const [value, setValue] = useState(from);

  useEffect(() => {
    let startTime: number | null = null;
    let frameId: number;

    const animate = (timestamp: number) => {
      if (!startTime) startTime = timestamp;
      const progress = Math.min((timestamp - startTime) / duration, 1);
      const eased = progress === 1 ? 1 : 1 - Math.pow(2, -10 * progress);
      setValue(from + (to - from) * eased);

      if (progress < 1) {
        frameId = requestAnimationFrame(animate);
      }
    };

    frameId = requestAnimationFrame(animate);
    return () => cancelAnimationFrame(frameId);
  }, [from, to, duration]);

  return value;
}

export default function App() {
  const [state, setState] = useState<MorphState>("idle");
  const [filePaths, setFilePaths] = useState<string[]>([]);
  const [selectedFormat, setSelectedFormat] = useState<string>("");
  const [hoveredBubble, setHoveredBubble] = useState<string | null>(null);
  const [shakingBubble, setShakingBubble] = useState<string | null>(null);
  const [lockNotice, setLockNotice] = useState<string>("");
  const [result, setResult] = useState<MorphResult | null>(null);
  const [errorMsg, setErrorMsg] = useState<string>("");
  const [copied, setCopied] = useState(false);
  const [isHoveringDone, setIsHoveringDone] = useState(false);

  // Impostazioni persistenti
  const [shortcutPreset, setShortcutPreset] = useState<number>(() =>
    Number(localStorage.getItem("morph_preset") ?? 0)
  );
  const [shortcutEnabled, setShortcutEnabled] = useState<boolean>(
    () => localStorage.getItem("morph_sc_enabled") !== "false"
  );
  const [autoGrabFile, setAutoGrabFile] = useState<boolean>(
    () => localStorage.getItem("morph_autograb") !== "false"
  );
  const [autoHideMode, setAutoHideMode] = useState<boolean>(
    () => localStorage.getItem("morph_autohide") !== "false"
  );
  const [autostart, setAutostart] = useState<boolean>(
    () => localStorage.getItem("morph_autostart") === "true"
  );
  const [outputMode, setOutputMode] = useState<string>(
    () => localStorage.getItem("morph_output_mode") || "same"
  );
  const [favoriteFormat, setFavoriteFormat] = useState<string>(
    () => localStorage.getItem("morph_fav_format") || "webp"
  );

  const autoHideModeRef = useRef(autoHideMode);
  autoHideModeRef.current = autoHideMode;
  const outputModeRef = useRef(outputMode);
  outputModeRef.current = outputMode;
  const filePathsRef = useRef(filePaths);
  filePathsRef.current = filePaths;

  useEffect(() => {
    localStorage.setItem("morph_preset", String(shortcutPreset));
    localStorage.setItem("morph_sc_enabled", String(shortcutEnabled));
    localStorage.setItem("morph_autograb", String(autoGrabFile));
    localStorage.setItem("morph_autohide", String(autoHideMode));
    localStorage.setItem("morph_autostart", String(autostart));
    localStorage.setItem("morph_output_mode", outputMode);
    localStorage.setItem("morph_fav_format", favoriteFormat);

    invoke("update_settings", {
      preset: shortcutPreset,
      shortcutEnabled,
      autoGrab: autoGrabFile,
      autostart,
    });
  }, [
    shortcutPreset,
    shortcutEnabled,
    autoGrabFile,
    autoHideMode,
    autostart,
    outputMode,
    favoriteFormat,
  ]);

  const primaryPath = filePaths[0] || "";
  const isBatch = filePaths.length > 1;
  const fileName = primaryPath ? primaryPath.split(/[\\/]/).pop() || "" : "File";
  const fileExt = fileName.includes(".")
    ? `.${fileName.split(".").pop()?.toUpperCase()}`
    : ".FILE";

  const category = detectCategory(fileExt);
  const wheelSlots = getWheelForFiles(filePaths);
  const wheelSlotsRef = useRef<BubbleSlot[]>(wheelSlots);
  wheelSlotsRef.current = wheelSlots;

  const hoveredBubbleRef = useRef<string | null>(null);
  hoveredBubbleRef.current = hoveredBubble;

  const animatedNewSize = useAnimatedNumber(
    result?.oldSize || 0,
    result?.newSize || 0
  );
  const targetDiffPercent =
    result && result.oldSize > 0
      ? Math.round(((result.newSize - result.oldSize) / result.oldSize) * 100)
      : 0;
  const animatedPercent = useAnimatedNumber(0, Math.abs(targetDiffPercent));

  const currentShortcutLabel =
    SHORTCUT_OPTIONS.find((s) => s.id === shortcutPreset)?.label || "Alt + M";

  // Eventi da Rust (Scorciatoia Globale e System Tray)
  useEffect(() => {
    let unlistenSummon: (() => void) | undefined;
    let unlistenSettings: (() => void) | undefined;

    listen<string[]>("shortcut-summon", (event) => {
      const grabbedFiles = event.payload;
      setLockNotice("");
      if (grabbedFiles && grabbedFiles.length > 0) {
        setFilePaths(grabbedFiles);
        setState("radial");
      } else {
        setState("idle");
      }
    }).then((fn) => {
      unlistenSummon = fn;
    });

    listen("open-settings", () => {
      setState("settings");
    }).then((fn) => {
      unlistenSettings = fn;
    });

    return () => {
      if (unlistenSummon) unlistenSummon();
      if (unlistenSettings) unlistenSettings();
    };
  }, []);

  const triggerLockRejection = (slot: BubbleSlot) => {
    setShakingBubble(slot.id);
    setLockNotice(slot.lockReason || "Conversione non supportata");
    setTimeout(() => setShakingBubble(null), 450);
  };

  const runConversion = async (pathsToConvert: string[], slot: BubbleSlot) => {
    if (!slot.enabled) {
      triggerLockRejection(slot);
      return;
    }

    setSelectedFormat(slot.label);
    setState("morphing");
    setHoveredBubble(null);
    setLockNotice("");
    setCopied(false);

    const startTime = Date.now();
    try {
      const res = await invoke<MorphResult>("morph_files", {
        filePaths: pathsToConvert,
        targetFormat: slot.id,
        outputMode: outputModeRef.current,
      });

      const elapsed = Date.now() - startTime;
      const remaining = Math.max(0, 480 - elapsed);

      setTimeout(() => {
        setResult(res);
        setState("done");
      }, remaining);
    } catch (err) {
      setErrorMsg(String(err));
      setState("error");
    }
  };

  // Scorciatoie da tastiera: ESC per chiudere, 1-8 per le bolle, INVIO per il formato Preferito ★
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (state === "radial" || state === "settings" || state === "done") {
          setState("idle");
          if (autoHideModeRef.current) invoke("hide_window");
        } else if (autoHideModeRef.current) {
          invoke("hide_window");
        }
      } else if (state === "radial") {
        if (e.key === "Enter") {
          const favSlot =
            wheelSlotsRef.current.find(
              (s) => s.id === favoriteFormat && s.enabled
            ) || wheelSlotsRef.current.find((s) => s.enabled);
          if (favSlot) runConversion(filePathsRef.current, favSlot);
        } else if (e.key >= "1" && e.key <= "8") {
          const idx = Number(e.key) - 1;
          const slot = wheelSlotsRef.current[idx];
          if (slot) runConversion(filePathsRef.current, slot);
        }
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [state, favoriteFormat]);

  useEffect(() => {
    if (state !== "done" || isHoveringDone) return;
    const timer = setTimeout(() => {
      setState("idle");
      if (autoHideModeRef.current) {
        setTimeout(() => invoke("hide_window"), 250);
      }
    }, 4500);
    return () => clearTimeout(timer);
  }, [state, isHoveringDone]);

  const handleCopyFile = async () => {
    if (!result) return;
    await invoke("copy_file_to_clipboard", { paths: result.outputPaths });
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  // Drag & Drop Multi-File Nativo
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let isCancelled = false;
    let lastEnterTime = 0;

    getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;

        if (payload.type === "enter") {
          lastEnterTime = Date.now();
          if (payload.paths && payload.paths.length > 0) {
            setFilePaths(payload.paths);
            setLockNotice("");
            setState("radial");
          }
        } else if (payload.type === "over") {
          const scale = window.devicePixelRatio || 1;
          const mouseX = payload.position.x / scale;
          const mouseY = payload.position.y / scale;

          const currentSlots = wheelSlotsRef.current;
          let foundActive: string | null = null;
          let hoveredLockedSlot: BubbleSlot | null = null;

          currentSlots.forEach((slot, i) => {
            const angle =
              (i * 2 * Math.PI) / currentSlots.length - Math.PI / 2;
            const bx = CENTER + Math.cos(angle) * RADIUS;
            const by = CENTER + Math.sin(angle) * RADIUS;
            const dist = Math.hypot(mouseX - bx, mouseY - by);

            if (dist < 32) {
              if (slot.enabled) {
                foundActive = slot.id;
              } else {
                hoveredLockedSlot = slot;
              }
            }
          });

          setHoveredBubble(foundActive);
          if (hoveredLockedSlot) {
            setLockNotice(
              (hoveredLockedSlot as BubbleSlot).lockReason || "Bloccato"
            );
          } else {
            setLockNotice("");
          }
        } else if (payload.type === "drop") {
          if (payload.paths && payload.paths.length > 0) {
            setFilePaths(payload.paths);
            const activeId = hoveredBubbleRef.current;
            const currentSlots = wheelSlotsRef.current;

            if (activeId) {
              const slot = currentSlots.find((s) => s.id === activeId);
              if (slot && slot.enabled) {
                runConversion(payload.paths, slot);
                return;
              }
            }
            setState("radial");
          }
        } else if (payload.type === "leave") {
          if (Date.now() - lastEnterTime < 180) return;
          setHoveredBubble(null);
          setLockNotice("");
          setState((prev) => (prev === "radial" ? "idle" : prev));
        }
      })
      .then((fn) => {
        if (isCancelled) fn();
        else unlisten = fn;
      });

    return () => {
      isCancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  const podDimensions = {
    idle: { width: 245, height: 60, borderRadius: 30 },
    radial: { width: 336, height: 336, borderRadius: 48 },
    morphing: { width: 195, height: 195, borderRadius: 42 },
    done: { width: 285, height: 185, borderRadius: 28 },
    error: { width: 265, height: 155, borderRadius: 28 },
    settings: { width: 310, height: 330, borderRadius: 28 },
  }[state];

  const prevAreaRef = useRef<number>(255 * 70);
  useEffect(() => {
    const winBounds = {
      idle: { width: 255, height: 70 },
      radial: { width: 350, height: 350 },
      morphing: { width: 210, height: 210 },
      done: { width: 295, height: 195 },
      error: { width: 275, height: 165 },
      settings: { width: 322, height: 342 },
    }[state];

    const newArea = winBounds.width * winBounds.height;
    const isExpanding = newArea >= prevAreaRef.current;
    prevAreaRef.current = newArea;

    const delay = isExpanding ? 0 : 220;
    const timer = setTimeout(() => {
      invoke("sync_window_bounds", winBounds);
    }, delay);

    return () => clearTimeout(timer);
  }, [state]);

  const CategoryIcon = isBatch
    ? Layers
    : {
        image: FileImage,
        document: FileText,
        data: FileSpreadsheet,
        archive: FolderArchive,
        unknown: FileText,
      }[category];

  return (
    <div className="w-screen h-screen flex items-center justify-center bg-transparent">
      <motion.div
        animate={podDimensions}
        transition={{ type: "spring", stiffness: 380, damping: 28 }}
        onMouseEnter={() => state === "done" && setIsHoveringDone(true)}
        onMouseLeave={() => state === "done" && setIsHoveringDone(false)}
        className="relative flex flex-col items-center justify-center bg-zinc-950/92 backdrop-blur-xl border border-white/12 text-white shadow-2xl overflow-hidden"
      >
        {/* Barra superiore */}
        <div
          data-tauri-drag-region
          className="absolute top-0 inset-x-0 h-7 flex items-center justify-between px-3 cursor-grab active:cursor-grabbing z-30 opacity-50 hover:opacity-100 transition-opacity"
        >
          <button
            onClick={() =>
              setState((prev) => (prev === "settings" ? "idle" : "settings"))
            }
            className={`transition-colors cursor-pointer ${
              state === "settings"
                ? "text-emerald-400"
                : "text-zinc-400 hover:text-white"
            }`}
            title="Impostazioni"
          >
            <Settings className="w-3.5 h-3.5" />
          </button>

          <GripHorizontal className="w-4 h-4 text-zinc-500 pointer-events-none" />

          <div className="flex items-center gap-2">
            <button
              onClick={() => invoke("hide_window")}
              className="text-zinc-400 hover:text-emerald-400 transition-colors cursor-pointer"
              title={`Nascondi nella System Tray (${currentShortcutLabel})`}
            >
              <EyeOff className="w-3.5 h-3.5" />
            </button>
            <button
              onClick={() => getCurrentWindow().close()}
              className="text-zinc-500 hover:text-rose-400 transition-colors cursor-pointer"
              title="Chiudi Morph"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          </div>
        </div>

        <AnimatePresence mode="wait">
          {/* STATO 1: IDLE */}
          {state === "idle" && (
            <motion.div
              key="idle"
              initial={{ opacity: 0, scale: 0.9 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.85 }}
              transition={{ duration: 0.15 }}
              className="flex flex-col items-center justify-center px-4 pt-3 w-full h-full"
            >
              <div className="flex items-center gap-2">
                <Sparkles className="w-4 h-4 text-emerald-400 animate-pulse shrink-0" />
                <span className="text-xs font-medium tracking-wide text-zinc-200">
                  Drop file o premi{" "}
                  <kbd className="px-1.5 py-0.5 text-[10px] bg-white/10 rounded border border-white/15 text-emerald-300 font-semibold">
                    {currentShortcutLabel}
                  </kbd>
                </span>
              </div>
            </motion.div>
          )}

          {/* STATO IMPOSTAZIONI COMPLETO */}
          {state === "settings" && (
            <motion.div
              key="settings"
              initial={{ opacity: 0, scale: 0.92 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.92 }}
              transition={{ duration: 0.15 }}
              className="flex flex-col justify-between w-full h-full px-5 pt-8 pb-3.5 text-left"
            >
              <div>
                <div className="flex items-center gap-1.5 text-emerald-400 mb-2.5">
                  <Keyboard className="w-4 h-4" />
                  <span className="text-xs font-bold uppercase tracking-wider">
                    Impostazioni Morph v1.0
                  </span>
                </div>

                {/* Scelta Scorciatoia */}
                <div className="mb-2.5">
                  <span className="text-[10px] text-zinc-400 block mb-1">
                    Scorciatoia globale:
                  </span>
                  <div className="grid grid-cols-2 gap-1">
                    {SHORTCUT_OPTIONS.map((opt) => (
                      <button
                        key={opt.id}
                        onClick={() => setShortcutPreset(opt.id)}
                        className={`text-[11px] py-1 px-2 rounded-lg border font-medium transition-colors cursor-pointer ${
                          shortcutPreset === opt.id
                            ? "bg-emerald-500/20 border-emerald-400 text-emerald-300"
                            : "bg-white/5 border-white/10 text-zinc-400 hover:text-white"
                        }`}
                      >
                        {opt.label}
                      </button>
                    ))}
                  </div>
                </div>

                {/* Scelta Cartella di Destinazione */}
                <div className="mb-2.5">
                  <span className="text-[10px] text-zinc-400 block mb-1">
                    Salva file convertiti in:
                  </span>
                  <div className="grid grid-cols-3 gap-1">
                    {OUTPUT_MODES.map((m) => (
                      <button
                        key={m.id}
                        onClick={() => setOutputMode(m.id)}
                        className={`text-[10px] py-1 px-1.5 rounded-lg border font-medium transition-colors cursor-pointer ${
                          outputMode === m.id
                            ? "bg-emerald-500/20 border-emerald-400 text-emerald-300"
                            : "bg-white/5 border-white/10 text-zinc-400 hover:text-white"
                        }`}
                      >
                        {m.label}
                      </button>
                    ))}
                  </div>
                </div>

                {/* Opzioni */}
                <div className="space-y-1.5 mt-2">
                  <label className="flex items-center justify-between cursor-pointer">
                    <span className="text-[11px] text-zinc-300">
                      Attiva scorciatoia da tastiera
                    </span>
                    <input
                      type="checkbox"
                      checked={shortcutEnabled}
                      onChange={(e) => setShortcutEnabled(e.target.checked)}
                      className="accent-emerald-400 w-3.5 h-3.5 cursor-pointer"
                    />
                  </label>

                  <label className="flex items-center justify-between cursor-pointer">
                    <span className="text-[11px] text-zinc-300">
                      Cattura file selezionati in Esplora File
                    </span>
                    <input
                      type="checkbox"
                      checked={autoGrabFile}
                      onChange={(e) => setAutoGrabFile(e.target.checked)}
                      className="accent-emerald-400 w-3.5 h-3.5 cursor-pointer"
                    />
                  </label>

                  <label className="flex items-center justify-between cursor-pointer">
                    <span className="text-[11px] text-zinc-300">
                      Nascondi nella Tray dopo conversione
                    </span>
                    <input
                      type="checkbox"
                      checked={autoHideMode}
                      onChange={(e) => setAutoHideMode(e.target.checked)}
                      className="accent-emerald-400 w-3.5 h-3.5 cursor-pointer"
                    />
                  </label>

                  <label className="flex items-center justify-between cursor-pointer">
                    <span className="text-[11px] text-zinc-300">
                      Avvia automaticamente con Windows
                    </span>
                    <input
                      type="checkbox"
                      checked={autostart}
                      onChange={(e) => setAutostart(e.target.checked)}
                      className="accent-emerald-400 w-3.5 h-3.5 cursor-pointer"
                    />
                  </label>
                </div>
              </div>

              <div className="flex items-center justify-between pt-2 border-t border-white/10">
                <button
                  onClick={() => {
                    setState("idle");
                    invoke("hide_window");
                  }}
                  className="text-[11px] text-emerald-300 hover:underline cursor-pointer"
                >
                  Minimizza in Tray
                </button>
                <button
                  onClick={() => setState("idle")}
                  className="text-[11px] bg-white/10 hover:bg-white/20 px-3 py-1 rounded-full cursor-pointer"
                >
                  Salva
                </button>
              </div>
            </motion.div>
          )}

          {/* STATO 2: RADIAL MENU CON SUPPORTO BATCH, PREFERITI ★ E TASTI 1-8 */}
          {state === "radial" && (
            <motion.div
              key="radial"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0, scale: 0.8 }}
              className="relative w-full h-full flex items-center justify-center"
            >
              {/* Nucleo Centrale (Mostra Singolo File o Conteggio Batch) */}
              <motion.button
                initial={{ scale: 0 }}
                animate={{ scale: 1 }}
                transition={{ type: "spring", stiffness: 450, damping: 22 }}
                onClick={() => {
                  setState("idle");
                  if (autoHideMode) invoke("hide_window");
                }}
                className="flex flex-col items-center justify-center w-21 h-21 px-2 rounded-2xl bg-white/5 border border-white/10 hover:bg-white/10 transition-colors cursor-pointer z-10"
                title="Clicca per annullare (Esc) o premi Invio per convertire nel formato ★"
              >
                <CategoryIcon className="w-6 h-6 text-emerald-400 shrink-0" />
                <span className="text-[10px] font-bold text-zinc-200 mt-1">
                  {isBatch ? `${filePaths.length} FILE` : fileExt}
                </span>
                <span className="text-[9px] text-zinc-400 truncate w-full text-center">
                  {isBatch ? `Batch (${fileExt})` : fileName}
                </span>
              </motion.button>

              {wheelSlots.map((slot, i) => {
                const angle =
                  (i * 2 * Math.PI) / wheelSlots.length - Math.PI / 2;
                const x = Math.cos(angle) * RADIUS;
                const y = Math.sin(angle) * RADIUS;
                const isHighlighted = hoveredBubble === slot.id && slot.enabled;
                const isShaking = shakingBubble === slot.id;
                const isFav = favoriteFormat === slot.id && slot.enabled;

                return (
                  <motion.button
                    key={slot.id}
                    initial={{ x: 0, y: 0, scale: 0, opacity: 0 }}
                    animate={{
                      x: isShaking ? [x - 6, x + 6, x - 4, x + 4, x] : x,
                      y,
                      scale: isHighlighted ? 1.24 : 1,
                      opacity: slot.enabled ? 1 : 0.32,
                    }}
                    exit={{ x: 0, y: 0, scale: 0, opacity: 0 }}
                    whileHover={slot.enabled ? { scale: 1.18 } : { scale: 0.96 }}
                    whileTap={slot.enabled ? { scale: 0.92 } : { scale: 0.9 }}
                    transition={{
                      type: isShaking ? "tween" : "spring",
                      duration: isShaking ? 0.32 : undefined,
                      stiffness: 420,
                      damping: 22,
                      delay: isShaking ? 0 : i * 0.02,
                    }}
                    onMouseEnter={() =>
                      !slot.enabled &&
                      setLockNotice(slot.lockReason || "Formato bloccato")
                    }
                    onMouseLeave={() => setLockNotice("")}
                    onClick={() => runConversion(filePaths, slot)}
                    onContextMenu={(e) => {
                      e.preventDefault();
                      if (slot.enabled) setFavoriteFormat(slot.id);
                    }}
                    title={
                      slot.enabled
                        ? `Tasto [ ${i + 1} ] • Tasto destro per impostare come ★ Preferito (Invio)`
                        : slot.lockReason
                    }
                    className={`absolute w-13 h-13 rounded-full flex flex-col items-center justify-center text-[11px] font-semibold shadow-lg transition-colors ${
                      !slot.enabled
                        ? "bg-zinc-900/60 border border-dashed border-white/15 text-zinc-500 cursor-not-allowed"
                        : isHighlighted
                        ? `${slot.color} bg-zinc-800 border-2 shadow-emerald-500/20 cursor-pointer`
                        : `bg-zinc-900 border border-white/15 text-zinc-200 hover:${slot.color} cursor-pointer`
                    }`}
                  >
                    <div className="flex items-center gap-0.5">
                      <span>{slot.label}</span>
                      {isFav && (
                        <Star className="w-2.5 h-2.5 text-amber-400 fill-amber-400" />
                      )}
                    </div>
                    {!slot.enabled ? (
                      <Lock className="w-2.5 h-2.5 text-rose-400/80 mt-0.5" />
                    ) : (
                      <span className="text-[8px] text-zinc-500 font-normal leading-none mt-0.5">
                        [{i + 1}]
                      </span>
                    )}
                  </motion.button>
                );
              })}

              <AnimatePresence>
                {lockNotice ? (
                  <motion.div
                    key="lock-notice"
                    initial={{ opacity: 0, y: 6 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, y: 4 }}
                    className="absolute bottom-2 px-2.5 py-0.5 rounded-full bg-rose-500/15 border border-rose-500/30 text-[10px] text-rose-300 font-medium pointer-events-none"
                  >
                    🔒 {lockNotice}
                  </motion.div>
                ) : category === "document" ? (
                  <motion.div
                    key="doc-scope"
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    exit={{ opacity: 0 }}
                    className="absolute bottom-2 px-2.5 py-0.5 rounded-full bg-white/5 border border-white/10 text-[9px] text-zinc-400 pointer-events-none"
                  >
                    Modalità documento: estrazione e conversione testo
                  </motion.div>
                ) : null}
              </AnimatePresence>
            </motion.div>
          )}

          {/* STATO 3: MORPHING */}
          {state === "morphing" && (
            <motion.div
              key="morphing"
              initial={{ opacity: 0, scale: 0.8 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.8 }}
              className="flex flex-col items-center gap-3"
            >
              <motion.div
                animate={{ rotate: 360 }}
                transition={{ repeat: Infinity, duration: 0.7, ease: "linear" }}
                className="w-11 h-11 rounded-full border-2 border-white/10 border-t-emerald-400"
              />
              <span className="text-xs text-zinc-300 font-medium">
                Morphing {isBatch ? `${filePaths.length} file ` : ""}in{" "}
                {selectedFormat}...
              </span>
            </motion.div>
          )}

          {/* STATO 4: DONE */}
          {state === "done" && result && (
            <motion.div
              key="done"
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -10 }}
              transition={{ type: "spring", stiffness: 400, damping: 25 }}
              className="flex flex-col items-center justify-center px-4 pt-4 text-center w-full h-full relative"
            >
              <div className="flex items-center gap-1.5 text-emerald-400 mb-0.5">
                <Check className="w-4 h-4 stroke-[2.5]" />
                <span className="text-xs font-semibold uppercase tracking-wider">
                  {result.filesCount > 1
                    ? `${result.filesCount} File Pronti (${selectedFormat})`
                    : `Pronto (${selectedFormat})`}
                </span>
              </div>

              <div className="text-base font-bold text-white tracking-tight tabular-nums">
                {formatBytes(result.oldSize)}{" "}
                <span className="text-zinc-500">➔</span>{" "}
                <span className="text-emerald-400">
                  {formatBytes(animatedNewSize)}
                </span>
              </div>

              <span className="text-[11px] text-zinc-400 font-medium mb-3 tabular-nums">
                {targetDiffPercent <= 0
                  ? `Risparmiato il ${Math.round(animatedPercent)}% di spazio`
                  : `Convertito con successo`}
              </span>

              <div className="flex items-center gap-1.5">
                <motion.button
                  whileTap={{ scale: 0.93 }}
                  onClick={handleCopyFile}
                  className="flex items-center gap-1.5 text-xs font-medium text-zinc-950 bg-emerald-400 hover:bg-emerald-300 px-3 py-1.5 rounded-full transition-colors cursor-pointer"
                >
                  <AnimatePresence mode="wait" initial={false}>
                    {copied ? (
                      <motion.span
                        key="check"
                        initial={{ scale: 0, rotate: -45 }}
                        animate={{ scale: 1, rotate: 0 }}
                        exit={{ scale: 0 }}
                        className="flex items-center gap-1"
                      >
                        <Check className="w-3.5 h-3.5 stroke-[2.5]" /> Copiato!
                      </motion.span>
                    ) : (
                      <motion.span
                        key="copy"
                        initial={{ scale: 0 }}
                        animate={{ scale: 1 }}
                        exit={{ scale: 0 }}
                        className="flex items-center gap-1"
                      >
                        <Copy className="w-3.5 h-3.5" /> Copia
                      </motion.span>
                    )}
                  </AnimatePresence>
                </motion.button>

                <button
                  onClick={() =>
                    invoke("reveal_in_folder", { paths: result.outputPaths })
                  }
                  className="flex items-center gap-1 text-xs text-zinc-200 bg-white/8 hover:bg-white/15 border border-white/10 px-2.5 py-1.5 rounded-full transition-colors cursor-pointer"
                >
                  <FolderOpen className="w-3.5 h-3.5" /> Cartella
                </button>

                <button
                  onClick={() => setState("idle")}
                  className="flex items-center justify-center text-xs text-zinc-400 hover:text-white bg-white/5 hover:bg-white/10 p-1.5 rounded-full transition-colors cursor-pointer"
                  title="Nuovo file"
                >
                  <RotateCcw className="w-3.5 h-3.5" />
                </button>
              </div>

              {!isHoveringDone && (
                <motion.div
                  initial={{ scaleX: 1 }}
                  animate={{ scaleX: 0 }}
                  transition={{ duration: 4.5, ease: "linear" }}
                  className="absolute bottom-0 inset-x-0 h-[2px] bg-emerald-400/60 origin-left"
                />
              )}
            </motion.div>
          )}

          {/* STATO ERRORE */}
          {state === "error" && (
            <motion.div
              key="error"
              initial={{ opacity: 0, scale: 0.9 }}
              animate={{ opacity: 1, scale: 1 }}
              className="flex flex-col items-center justify-center px-4 pt-3 text-center"
            >
              <AlertCircle className="w-5 h-5 text-rose-400 mb-1" />
              <span className="text-xs text-zinc-300 mb-2 line-clamp-2">
                {errorMsg || "Impossibile convertire questo file"}
              </span>
              <button
                onClick={() => setState("idle")}
                className="text-xs bg-white/10 hover:bg-white/20 px-3 py-1 rounded-full cursor-pointer"
              >
                Riprova
              </button>
            </motion.div>
          )}
        </AnimatePresence>
      </motion.div>
    </div>
  );
}