import React, { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Archive,
  ArrowLeft,
  BarChart3,
  CheckCircle2,
  Copy,
  FileSearch,
  Flame,
  FolderOpen,
  Gauge,
  HardDrive,
  Layers,
  Play,
  RotateCcw,
  Search,
  ShieldAlert,
  ShieldCheck,
  Snowflake,
  Sparkles,
  Square,
  Trash2,
  Zap,
} from "lucide-react";
import {
  BenchmarkReport,
  CompressionMode,
  GameDetailResponse,
  GameRecord,
  GlobalStats,
  ModeEstimate,
  ProgressEvent,
  ScanPreview,
} from "./types";
import { formatBytes, formatDate, formatModeLabel } from "./utils";

type Tab = "dashboard" | "compress" | "inspector";

export default function App() {
  const [tab, setTab] = useState<Tab>("dashboard");
  const [games, setGames] = useState<GameRecord[]>([]);
  const [stats, setStats] = useState<GlobalStats | null>(null);
  const [selectedDetail, setSelectedDetail] = useState<GameDetailResponse | null>(null);

  // Compress flow state
  const [folderPath, setFolderPath] = useState("");
  const [customName, setCustomName] = useState("");
  const [mode, setMode] = useState<CompressionMode>("max_crush");
  const [enableHardlinks, setEnableHardlinks] = useState(true);
  const [enableColdVault, setEnableColdVault] = useState(false);
  const [scanPreview, setScanPreview] = useState<ScanPreview | null>(null);
  const [benchmark, setBenchmark] = useState<BenchmarkReport | null>(null);
  const [isScanning, setIsScanning] = useState(false);
  const [isBenchRunning, setIsBenchRunning] = useState(false);
  const [isRunning, setIsRunning] = useState(false);
  const [isCancelling, setIsCancelling] = useState(false);
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [toastMsg, setToastMsg] = useState<string | null>(null);

  // Details filters
  const [auditSearch, setAuditSearch] = useState("");
  const [auditFilter, setAuditFilter] = useState("ALL");

  const showToast = (msg: string) => {
    setToastMsg(msg);
    setTimeout(() => setToastMsg(null), 4500);
  };

  const refreshLibrary = async () => {
    try {
      const [gList, gStats] = await Promise.all([
        invoke<GameRecord[]>("list_games"),
        invoke<GlobalStats>("get_global_stats"),
      ]);
      setGames(gList);
      setStats(gStats);
    } catch (e) {
      setErrorMsg(String(e));
    }
  };

  useEffect(() => {
    refreshLibrary();
    const unlistenPromise = listen<ProgressEvent>("compression-progress", (event) => {
      setProgress(event.payload);
    });
    return () => {
      unlistenPromise.then((fn) => fn());
    };
  }, []);

  const handlePickFolder = async () => {
    setErrorMsg(null);
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Choose Game Folder",
      });
      if (typeof selected === "string" && selected.length > 0) {
        setFolderPath(selected);
        await runFolderScan(selected);
      }
    } catch (e) {
      setErrorMsg(String(e));
    }
  };

  const runFolderScan = async (targetPath: string) => {
    if (!targetPath.trim()) return;
    setIsScanning(true);
    setErrorMsg(null);
    setScanPreview(null);
    setBenchmark(null);
    try {
      const preview = await invoke<ScanPreview>("scan_game_folder", { path: targetPath.trim() });
      setScanPreview(preview);
      setCustomName(preview.suggested_name);
      if (preview.anti_cheat_detected) {
        setEnableColdVault(false);
      }
    } catch (e) {
      setErrorMsg(String(e));
    } finally {
      setIsScanning(false);
    }
  };

  const handleEstimateSavings = async () => {
    if (!folderPath.trim()) return;
    setIsBenchRunning(true);
    setErrorMsg(null);
    try {
      const report = await invoke<BenchmarkReport>("estimate_all_modes", {
        path: folderPath.trim(),
      });
      setBenchmark(report);
    } catch (e) {
      setErrorMsg(String(e));
    } finally {
      setIsBenchRunning(false);
    }
  };

  const handleCancel = async () => {
    setIsCancelling(true);
    try {
      await invoke("cancel_operation");
    } catch (e) {
      setErrorMsg(String(e));
    }
  };

  const handleCompress = async () => {
    if (!folderPath.trim()) return;
    setIsRunning(true);
    setIsCancelling(false);
    setErrorMsg(null);
    setProgress({
      phase: "Starting",
      current_file: "Preparing files...",
      processed_files: 0,
      total_files: scanPreview?.total_files ?? 1,
      saved_so_far_bytes: 0,
      pct: 0,
    });
    try {
      const record = await invoke<GameRecord>("compress_game", {
        request: {
          path: folderPath.trim(),
          name: customName.trim() || null,
          mode,
          enable_hardlink_dedup: enableHardlinks,
          enable_cold_vault: enableColdVault && !(scanPreview?.anti_cheat_detected ?? false),
        },
      });
      await refreshLibrary();
      await openInspector(record.id);
      if (record.status === "cancelled") {
        showToast(
          `Compression cancelled safely. Saved ${formatBytes(record.saved_bytes)} on files already processed.`
        );
      } else {
        showToast(
          `Finished shrinking ${record.name} — Saved ${formatBytes(record.saved_bytes)} (${record.ratio_pct.toFixed(1)}%)`
        );
      }
    } catch (e) {
      setErrorMsg(String(e));
    } finally {
      setIsRunning(false);
      setIsCancelling(false);
    }
  };

  const openInspector = async (gameId: string) => {
    setErrorMsg(null);
    try {
      const detail = await invoke<GameDetailResponse>("get_game_details", { gameId });
      setSelectedDetail(detail);
      setAuditSearch("");
      setAuditFilter("ALL");
      setTab("inspector");
    } catch (e) {
      setErrorMsg(String(e));
    }
  };

  const handleRestore = async (game: GameRecord) => {
    setIsRunning(true);
    setIsCancelling(false);
    setErrorMsg(null);
    try {
      const res = await invoke<GameRecord>("restore_game", {
        gameId: game.id,
        breakHardlinks: true,
      });
      await refreshLibrary();
      if (selectedDetail?.game.id === game.id) {
        await openInspector(game.id);
      }
      if (res.status === "cancelled") {
        showToast("Restore stopped by user.");
      } else {
        showToast(`Restored ${game.name} to its original uncompressed files.`);
      }
    } catch (e) {
      setErrorMsg(String(e));
    } finally {
      setIsRunning(false);
      setIsCancelling(false);
    }
  };

  const handleLaunchExe = async (exePath: string | null) => {
    if (!exePath) {
      setErrorMsg("No game .exe was found automatically in this folder.");
      return;
    }
    try {
      await invoke("launch_game_exe", { exePath });
      showToast("Game launched directly from its .exe.");
    } catch (e) {
      setErrorMsg(String(e));
    }
  };

  const handleOpenFolder = async (path: string) => {
    try {
      await invoke("open_folder", { path });
    } catch (e) {
      setErrorMsg(String(e));
    }
  };

  const handleDeleteRecord = async (gameId: string) => {
    try {
      await invoke("delete_game_record", { gameId });
      await refreshLibrary();
      if (selectedDetail?.game.id === gameId) {
        setSelectedDetail(null);
        setTab("dashboard");
      }
      showToast("Removed from history (your game files were not touched).");
    } catch (e) {
      setErrorMsg(String(e));
    }
  };

  const getEstimateForMode = (m: CompressionMode): ModeEstimate | undefined => {
    return benchmark?.estimates.find((e) => e.mode === m);
  };

  const filteredAudits = useMemo(() => {
    if (!selectedDetail) return [];
    const q = auditSearch.trim().toLowerCase();
    return selectedDetail.audits.filter((a) => {
      const matchAction =
        auditFilter === "ALL" ||
        (auditFilter === "COMPRESSED"
          ? a.action === "Max Compressed" || a.action === "Fast Compressed"
          : auditFilter === "DEEP"
            ? a.action.startsWith("Deep Freeze") || a.action === "Archived Language"
            : a.action === auditFilter);
      const matchQuery =
        !q ||
        a.rel_path.toLowerCase().includes(q) ||
        a.category.toLowerCase().includes(q) ||
        a.action.toLowerCase().includes(q);
      return matchAction && matchQuery;
    });
  }, [selectedDetail, auditSearch, auditFilter]);

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-[#060911] text-slate-100">
      {/* Sidebar */}
      <aside className="flex w-64 flex-col border-r border-slate-800/80 bg-[#090d17] p-5">
        <div className="mb-8 flex items-center gap-3">
          <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-gradient-to-br from-emerald-500 to-cyan-500 shadow-lg shadow-emerald-500/20">
            <Flame className="h-5 w-5 text-slate-950" />
          </div>
          <div>
            <h1 className="text-base font-bold tracking-tight text-white">GameShrink</h1>
            <p className="text-[11px] font-medium text-emerald-400">Smart Game Compressor</p>
          </div>
        </div>

        <nav className="flex flex-col gap-1.5">
          <button
            onClick={() => setTab("dashboard")}
            className={`flex items-center gap-3 rounded-xl px-3.5 py-2.5 text-sm font-medium transition ${
              tab === "dashboard"
                ? "bg-emerald-500/15 text-emerald-300 ring-1 ring-emerald-500/30"
                : "text-slate-400 hover:bg-slate-800/60 hover:text-slate-200"
            }`}
          >
            <HardDrive className="h-4 w-4" />
            My Games
          </button>

          <button
            onClick={() => setTab("compress")}
            className={`flex items-center gap-3 rounded-xl px-3.5 py-2.5 text-sm font-medium transition ${
              tab === "compress"
                ? "bg-emerald-500/15 text-emerald-300 ring-1 ring-emerald-500/30"
                : "text-slate-400 hover:bg-slate-800/60 hover:text-slate-200"
            }`}
          >
            <Zap className="h-4 w-4" />
            Compress a Game
          </button>

          {selectedDetail && (
            <button
              onClick={() => setTab("inspector")}
              className={`flex items-center gap-3 rounded-xl px-3.5 py-2.5 text-sm font-medium transition ${
                tab === "inspector"
                  ? "bg-cyan-500/15 text-cyan-300 ring-1 ring-cyan-500/30"
                  : "text-slate-400 hover:bg-slate-800/60 hover:text-slate-200"
              }`}
            >
              <Layers className="h-4 w-4" />
              Game Details
            </button>
          )}
        </nav>

        {/* Total Saved Widget */}
        <div className="mt-auto rounded-2xl border border-emerald-500/20 bg-gradient-to-b from-emerald-500/10 to-slate-900/60 p-4">
          <p className="text-[11px] font-semibold uppercase tracking-wider text-emerald-400">
            Total Space Saved
          </p>
          <p className="mt-1 text-2xl font-bold text-white">
            {formatBytes(stats?.total_saved_bytes ?? 0)}
          </p>
          <p className="mt-1 text-xs text-slate-400">
            Across {stats?.total_games ?? 0} game{(stats?.total_games ?? 0) === 1 ? "" : "s"}
          </p>
          <div className="mt-3 space-y-1 text-[11px] text-slate-400">
            <div className="flex justify-between">
              <span>Playable Compression:</span>
              <span className="text-slate-200">{formatBytes(stats?.total_lzx_saved_bytes ?? 0)}</span>
            </div>
            <div className="flex justify-between">
              <span>Duplicate Files:</span>
              <span className="text-amber-300">{formatBytes(stats?.total_dedup_saved_bytes ?? 0)}</span>
            </div>
            <div className="flex justify-between">
              <span>Deep Freeze:</span>
              <span className="text-indigo-300">{formatBytes(stats?.total_vault_saved_bytes ?? 0)}</span>
            </div>
          </div>
        </div>
      </aside>

      {/* Main Area */}
      <main className="relative flex-1 overflow-y-auto p-8">
        {toastMsg && (
          <div className="mb-5 flex items-center gap-2.5 rounded-xl border border-emerald-500/30 bg-emerald-500/10 px-4 py-3 text-sm text-emerald-200">
            <CheckCircle2 className="h-4 w-4 shrink-0 text-emerald-400" />
            <span>{toastMsg}</span>
          </div>
        )}
        {errorMsg && (
          <div className="mb-5 flex items-center justify-between rounded-xl border border-rose-500/30 bg-rose-500/10 px-4 py-3 text-sm text-rose-200">
            <span>{errorMsg}</span>
            <button onClick={() => setErrorMsg(null)} className="text-xs underline">
              Dismiss
            </button>
          </div>
        )}

        {/* Live Progress Bar with CANCEL Button */}
        {isRunning && progress && (
          <div className="mb-6 rounded-2xl border border-emerald-500/30 bg-[#0c1322] p-5 shadow-xl shadow-emerald-950/30">
            <div className="flex items-center justify-between gap-4">
              <div className="min-w-0 flex-1">
                <p className="text-xs font-semibold uppercase tracking-wider text-emerald-400">
                  {isCancelling ? "Cancelling safely..." : progress.phase}
                </p>
                <p className="mt-1 truncate text-xs text-slate-400">{progress.current_file}</p>
              </div>
              <div className="flex items-center gap-4">
                <div className="text-right">
                  <p className="text-lg font-bold text-white">{progress.pct.toFixed(1)}%</p>
                  <p className="text-xs text-emerald-400">
                    Saved so far: {formatBytes(progress.saved_so_far_bytes)}
                  </p>
                </div>
                <button
                  onClick={handleCancel}
                  disabled={isCancelling}
                  className="flex items-center gap-1.5 rounded-xl bg-rose-500/20 px-4 py-2 text-xs font-bold text-rose-300 ring-1 ring-rose-500/40 transition hover:bg-rose-500/30 disabled:opacity-50"
                >
                  <Square className="h-3.5 w-3.5 fill-current" />
                  {isCancelling ? "Stopping..." : "Cancel"}
                </button>
              </div>
            </div>
            <div className="mt-3 h-2.5 w-full overflow-hidden rounded-full bg-slate-800">
              <div
                className="h-full rounded-full bg-gradient-to-r from-emerald-500 to-cyan-400 transition-all duration-200"
                style={{ width: `${Math.max(2, progress.pct)}%` }}
              />
            </div>
            <p className="mt-2 text-[11px] text-slate-500">
              {progress.processed_files.toLocaleString()} / {progress.total_files.toLocaleString()} files processed
            </p>
          </div>
        )}

        {/* TAB 1: MY GAMES DASHBOARD */}
        {tab === "dashboard" && (
          <div className="space-y-6">
            <div className="flex items-center justify-between">
              <div>
                <h2 className="text-2xl font-bold text-white">My Compressed Games</h2>
                <p className="mt-1 text-sm text-slate-400">
                  Track before & after sizes, launch playable games directly from their .exe, or restore anytime.
                </p>
              </div>
              <button
                onClick={() => setTab("compress")}
                className="flex items-center gap-2 rounded-xl bg-emerald-500 px-4 py-2.5 text-sm font-semibold text-slate-950 transition hover:bg-emerald-400"
              >
                <Zap className="h-4 w-4" />
                Compress a Game
              </button>
            </div>

            <div className="grid grid-cols-4 gap-4">
              <div className="rounded-2xl border border-slate-800/80 bg-[#0b101b] p-4">
                <p className="text-xs text-slate-400">Total Space Saved</p>
                <p className="mt-1.5 text-2xl font-bold text-emerald-400">
                  {formatBytes(stats?.total_saved_bytes ?? 0)}
                </p>
                <p className="mt-1 text-[11px] text-slate-500">
                  {formatBytes(stats?.total_before_bytes ?? 0)} → {formatBytes(stats?.total_after_bytes ?? 0)}
                </p>
              </div>

              <div className="rounded-2xl border border-slate-800/80 bg-[#0b101b] p-4">
                <p className="text-xs text-slate-400">Average Size Reduction</p>
                <p className="mt-1.5 text-2xl font-bold text-cyan-400">
                  {(stats?.avg_ratio_pct ?? 0).toFixed(1)}%
                </p>
                <p className="mt-1 text-[11px] text-slate-500">Real drive space freed</p>
              </div>

              <div className="rounded-2xl border border-slate-800/80 bg-[#0b101b] p-4">
                <p className="text-xs text-slate-400">Deep Freeze Savings</p>
                <p className="mt-1.5 text-2xl font-bold text-indigo-300">
                  {formatBytes(stats?.total_vault_saved_bytes ?? 0)}
                </p>
                <p className="mt-1 text-[11px] text-slate-500">Stored games & unused voices</p>
              </div>

              <div className="rounded-2xl border border-slate-800/80 bg-[#0b101b] p-4">
                <p className="text-xs text-slate-400">Duplicate Files Merged</p>
                <p className="mt-1.5 text-2xl font-bold text-amber-300">
                  {(stats?.total_files_hardlinked ?? 0).toLocaleString()}
                </p>
                <p className="mt-1 text-[11px] text-slate-500">
                  {formatBytes(stats?.total_dedup_saved_bytes ?? 0)} freed with 0 FPS loss
                </p>
              </div>
            </div>

            {games.length === 0 ? (
              <div className="flex flex-col items-center justify-center rounded-2xl border border-dashed border-slate-800 bg-[#0b101b]/50 py-16 text-center">
                <HardDrive className="mb-3 h-10 w-10 text-slate-600" />
                <h3 className="text-base font-semibold text-slate-200">No compressed games yet</h3>
                <p className="mt-1 max-w-md text-sm text-slate-400">
                  Pick any game folder to see how much space each option will save, then shrink it with one click.
                </p>
                <button
                  onClick={() => setTab("compress")}
                  className="mt-5 rounded-xl bg-emerald-500/15 px-4 py-2 text-sm font-medium text-emerald-300 ring-1 ring-emerald-500/30 hover:bg-emerald-500/25"
                >
                  Compress Your First Game
                </button>
              </div>
            ) : (
              <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
                {games.map((g) => {
                  const keepPct = g.before_bytes > 0 ? (g.after_bytes / g.before_bytes) * 100 : 100;
                  const isArchived = g.status === "archived";
                  return (
                    <div
                      key={g.id}
                      className="rounded-2xl border border-slate-800/80 bg-[#0b101b] p-5 transition hover:border-slate-700"
                    >
                      <div className="flex items-start justify-between gap-3">
                        <div className="min-w-0">
                          <div className="flex flex-wrap items-center gap-2">
                            <h3 className="truncate text-base font-bold text-white">{g.name}</h3>
                            <span
                              className={`rounded-md px-2 py-0.5 text-[10px] font-semibold ${
                                isArchived
                                  ? "bg-indigo-500/20 text-indigo-300"
                                  : "bg-emerald-500/15 text-emerald-300"
                              }`}
                            >
                              {formatModeLabel(g.mode)}
                            </span>
                            {g.status === "cancelled" && (
                              <span className="rounded-md bg-rose-500/15 px-2 py-0.5 text-[10px] font-semibold text-rose-300">
                                Partial (Cancelled)
                              </span>
                            )}
                            {g.anti_cheat_detected && (
                              <span
                                title={`Detected: ${g.anti_cheat_name}`}
                                className="flex items-center gap-1 rounded-md bg-amber-500/15 px-2 py-0.5 text-[10px] font-semibold text-amber-300"
                              >
                                <ShieldCheck className="h-3 w-3" />
                                Anti-Cheat Safe
                              </span>
                            )}
                          </div>
                          <p className="mt-1 truncate text-xs text-slate-500">{g.path}</p>
                        </div>
                        <div className="text-right">
                          <p className="text-lg font-bold text-emerald-400">
                            -{g.ratio_pct.toFixed(1)}%
                          </p>
                          <p className="text-[11px] text-slate-400">
                            Saved {formatBytes(g.saved_bytes)}
                          </p>
                        </div>
                      </div>

                      <div className="mt-4">
                        <div className="mb-1.5 flex justify-between text-xs">
                          <span className="text-slate-400">
                            Before: <strong className="text-slate-200">{formatBytes(g.before_bytes)}</strong>
                          </span>
                          <span className="text-slate-400">
                            After: <strong className="text-emerald-300">{formatBytes(g.after_bytes)}</strong>
                          </span>
                        </div>
                        <div className="h-2 w-full overflow-hidden rounded-full bg-slate-800">
                          <div
                            className={`h-full rounded-full ${
                              isArchived
                                ? "bg-gradient-to-r from-indigo-500 to-cyan-400"
                                : "bg-gradient-to-r from-cyan-500 to-emerald-400"
                            }`}
                            style={{ width: `${Math.min(100, Math.max(4, keepPct))}%` }}
                          />
                        </div>
                      </div>

                      <div className="mt-4 flex items-center gap-2 border-t border-slate-800/80 pt-3.5">
                        {isArchived ? (
                          <button
                            onClick={() => handleRestore(g)}
                            disabled={isRunning}
                            className="flex items-center gap-1.5 rounded-lg bg-indigo-500/20 px-3 py-1.5 text-xs font-semibold text-indigo-200 hover:bg-indigo-500/30 disabled:opacity-40"
                          >
                            <RotateCcw className="h-3.5 w-3.5" />
                            Unpack to Play
                          </button>
                        ) : (
                          g.exe_path && (
                            <button
                              onClick={() => handleLaunchExe(g.exe_path)}
                              className="flex items-center gap-1.5 rounded-lg bg-emerald-500/15 px-3 py-1.5 text-xs font-semibold text-emerald-300 hover:bg-emerald-500/25"
                            >
                              <Play className="h-3.5 w-3.5" />
                              Play Game
                            </button>
                          )
                        )}
                        <button
                          onClick={() => openInspector(g.id)}
                          className="flex items-center gap-1.5 rounded-lg bg-slate-800 px-3 py-1.5 text-xs font-medium text-slate-200 hover:bg-slate-700"
                        >
                          <Layers className="h-3.5 w-3.5" />
                          Details
                        </button>
                        <button
                          onClick={() => handleOpenFolder(g.path)}
                          className="rounded-lg bg-slate-800/70 p-1.5 text-slate-400 hover:bg-slate-700 hover:text-white"
                          title="Open Game Folder"
                        >
                          <FolderOpen className="h-3.5 w-3.5" />
                        </button>
                        {!isArchived && (
                          <button
                            onClick={() => handleRestore(g)}
                            disabled={isRunning}
                            className="ml-auto flex items-center gap-1 rounded-lg bg-slate-800/70 px-2.5 py-1.5 text-xs text-slate-300 hover:bg-amber-500/20 hover:text-amber-200 disabled:opacity-40"
                          >
                            <RotateCcw className="h-3.5 w-3.5" />
                            Restore
                          </button>
                        )}
                        <button
                          onClick={() => handleDeleteRecord(g.id)}
                          className={`${isArchived ? "ml-auto" : ""} rounded-lg p-1.5 text-slate-500 hover:bg-rose-500/15 hover:text-rose-300`}
                          title="Remove from history"
                        >
                          <Trash2 className="h-3.5 w-3.5" />
                        </button>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        )}

        {/* TAB 2: COMPRESS A GAME */}
        {tab === "compress" && (
          <div className="mx-auto max-w-5xl space-y-6">
            <div>
              <h2 className="text-2xl font-bold text-white">Compress a Game</h2>
              <p className="mt-1 text-sm text-slate-400">
                Choose a game folder, estimate how much space each option will save, and start shrinking.
              </p>
            </div>

            {/* Folder Picker */}
            <div className="rounded-2xl border border-slate-800/80 bg-[#0b101b] p-5">
              <label className="text-xs font-semibold uppercase tracking-wider text-slate-400">
                1. Choose Game Folder
              </label>
              <div className="mt-2 flex gap-2.5">
                <input
                  type="text"
                  value={folderPath}
                  onChange={(e) => setFolderPath(e.target.value)}
                  placeholder="C:\Program Files (x86)\Steam\steamapps\common\YourGame"
                  className="flex-1 rounded-xl border border-slate-700/80 bg-[#060911] px-4 py-2.5 text-sm text-slate-100 placeholder-slate-600 focus:border-emerald-500 focus:outline-none"
                />
                <button
                  onClick={handlePickFolder}
                  className="flex items-center gap-2 rounded-xl bg-slate-800 px-4 py-2.5 text-sm font-medium text-slate-200 hover:bg-slate-700"
                >
                  <FolderOpen className="h-4 w-4" />
                  Browse
                </button>
                <button
                  onClick={() => runFolderScan(folderPath)}
                  disabled={isScanning || !folderPath.trim()}
                  className="flex items-center gap-2 rounded-xl bg-cyan-500/15 px-4 py-2.5 text-sm font-semibold text-cyan-300 ring-1 ring-cyan-500/30 hover:bg-cyan-500/25 disabled:opacity-40"
                >
                  <FileSearch className="h-4 w-4" />
                  {isScanning ? "Scanning..." : "Scan Folder"}
                </button>
              </div>
            </div>

            {scanPreview && (
              <div className="space-y-5">
                {/* Folder Summary + Estimate Savings Button */}
                <div className="rounded-2xl border border-slate-800/80 bg-[#0b101b] p-5">
                  <div className="flex flex-wrap items-center justify-between gap-4">
                    <div>
                      <input
                        type="text"
                        value={customName}
                        onChange={(e) => setCustomName(e.target.value)}
                        className="bg-transparent text-xl font-bold text-white focus:outline-none"
                      />
                      <p className="mt-0.5 text-xs text-slate-400">
                        {scanPreview.total_files.toLocaleString()} files • Game executable:{" "}
                        <span className="text-slate-200">
                          {scanPreview.detected_exe ? scanPreview.detected_exe.split(/[\\/]/).pop() : "Not detected"}
                        </span>
                      </p>
                    </div>

                    <div className="flex items-center gap-2.5">
                      <button
                        onClick={handleEstimateSavings}
                        disabled={isBenchRunning}
                        className="flex items-center gap-2 rounded-xl bg-indigo-500/20 px-4 py-2 text-xs font-semibold text-indigo-200 ring-1 ring-indigo-500/40 hover:bg-indigo-500/30 disabled:opacity-40"
                      >
                        <BarChart3 className="h-4 w-4" />
                        {isBenchRunning ? "Testing sample files..." : "Estimate Savings for Each Option"}
                      </button>

                      {scanPreview.anti_cheat_detected ? (
                        <div className="flex items-center gap-2 rounded-xl border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-200">
                          <ShieldAlert className="h-4 w-4 text-amber-400" />
                          <span>{scanPreview.anti_cheat_name} detected</span>
                        </div>
                      ) : (
                        <div className="flex items-center gap-2 rounded-xl border border-emerald-500/30 bg-emerald-500/10 px-3 py-2 text-xs text-emerald-200">
                          <ShieldCheck className="h-4 w-4 text-emerald-400" />
                          <span>No Anti-Cheat</span>
                        </div>
                      )}
                    </div>
                  </div>

                  <div className="mt-4 grid grid-cols-4 gap-3">
                    <div className="rounded-xl bg-[#060911] p-3.5">
                      <p className="text-[11px] text-slate-400">Current Size</p>
                      <p className="mt-1 text-lg font-bold text-white">
                        {formatBytes(scanPreview.current_disk_bytes)}
                      </p>
                    </div>
                    <div className="rounded-xl bg-[#060911] p-3.5">
                      <p className="text-[11px] text-slate-400">Compressible Files</p>
                      <p className="mt-1 text-lg font-bold text-emerald-400">
                        {formatBytes(scanPreview.compressible_bytes)}
                      </p>
                    </div>
                    <div className="rounded-xl bg-[#060911] p-3.5">
                      <p className="text-[11px] text-slate-400">Duplicate Files Found</p>
                      <p className="mt-1 text-lg font-bold text-amber-300">
                        {scanPreview.duplicate_files} ({formatBytes(scanPreview.duplicate_reclaimable_bytes)})
                      </p>
                    </div>
                    <div className="rounded-xl bg-[#060911] p-3.5">
                      <p className="text-[11px] text-slate-400">Already Compressed</p>
                      <p className="mt-1 text-lg font-bold text-slate-300">
                        {formatBytes(scanPreview.high_entropy_bytes)}
                      </p>
                    </div>
                  </div>
                </div>

                {/* Benchmark Comparison Table (Shown after clicking Estimate Savings) */}
                {benchmark && (
                  <div className="rounded-2xl border border-indigo-500/30 bg-[#0b1122] p-5">
                    <div className="mb-3 flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <BarChart3 className="h-4 w-4 text-indigo-400" />
                        <h3 className="text-sm font-bold text-white">
                          Estimated Space Saved by Option (Tested on {benchmark.sample_count} real files)
                        </h3>
                      </div>
                      <span className="text-xs text-slate-400">
                        Current size: <strong className="text-white">{formatBytes(benchmark.total_game_bytes)}</strong>
                      </span>
                    </div>
                    <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
                      {benchmark.estimates.map((est) => (
                        <div
                          key={est.mode}
                          onClick={() => setMode(est.mode as CompressionMode)}
                          className={`cursor-pointer rounded-xl border p-3.5 transition ${
                            mode === est.mode
                              ? "border-emerald-400 bg-emerald-500/10"
                              : "border-slate-800 bg-[#070b15] hover:border-slate-700"
                          }`}
                        >
                          <p className="text-xs font-bold text-white">{est.label}</p>
                          <p className="mt-1.5 text-lg font-bold text-emerald-400">
                            Save ~{formatBytes(est.estimated_saved_bytes)}
                          </p>
                          <p className="text-[11px] text-slate-400">
                            Final size: ~{formatBytes(est.estimated_after_bytes)} (-{est.estimated_saved_pct.toFixed(1)}%)
                          </p>
                        </div>
                      ))}
                    </div>
                  </div>
                )}

                {/* 2. Choose Compression Option */}
                <div className="rounded-2xl border border-slate-800/80 bg-[#0b101b] p-5">
                  <h3 className="text-xs font-semibold uppercase tracking-wider text-slate-400">
                    2. Choose Compression Option
                  </h3>
                  <div className="mt-3 grid grid-cols-2 gap-3 lg:grid-cols-4">
                    {(
                      [
                        {
                          id: "max_crush" as CompressionMode,
                          title: "Playable Max",
                          badge: "Recommended • Play Directly",
                          badgeColor: "bg-emerald-500/15 text-emerald-300",
                          borderActive: "border-emerald-500 bg-emerald-500/10",
                          icon: <Flame className="h-4 w-4 text-emerald-400" />,
                          desc: "Strongest compression while staying 100% playable from your .exe. Skips already-compressed files.",
                        },
                        {
                          id: "brute_max" as CompressionMode,
                          title: "Playable Extreme",
                          badge: "Play Directly",
                          badgeColor: "bg-cyan-500/15 text-cyan-300",
                          borderActive: "border-cyan-500 bg-cyan-500/10",
                          icon: <Sparkles className="h-4 w-4 text-cyan-400" />,
                          desc: "Forces maximum Windows compression on every single file to squeeze out every last MB.",
                        },
                        {
                          id: "adaptive_thermal" as CompressionMode,
                          title: "Playable Balanced",
                          badge: "Fastest Loading",
                          badgeColor: "bg-purple-500/15 text-purple-300",
                          borderActive: "border-purple-500 bg-purple-500/10",
                          icon: <Gauge className="h-4 w-4 text-purple-400" />,
                          desc: "Best for massive open-world games. Uses high compression on static files and fast compression on maps.",
                        },
                        {
                          id: "hyper_archive" as CompressionMode,
                          title: "Deep Freeze",
                          badge: "Smallest Size • Unpack to Play",
                          badgeColor: "bg-indigo-500/20 text-indigo-300",
                          borderActive: "border-indigo-400 bg-indigo-500/15",
                          icon: <Snowflake className="h-4 w-4 text-indigo-300" />,
                          desc: "Absolute maximum space savings for games you aren't playing right now. 1-click Unpack when ready.",
                        },
                      ] as const
                    ).map((opt) => {
                      const est = getEstimateForMode(opt.id);
                      return (
                        <button
                          key={opt.id}
                          onClick={() => setMode(opt.id)}
                          className={`flex flex-col rounded-xl border p-4 text-left transition ${
                            mode === opt.id
                              ? opt.borderActive
                              : "border-slate-800 bg-[#060911] hover:border-slate-700"
                          }`}
                        >
                          <div className="flex w-full items-center justify-between">
                            <span className="text-sm font-bold text-white">{opt.title}</span>
                            {opt.icon}
                          </div>
                          <span className={`mt-1.5 inline-block w-fit rounded px-1.5 py-0.5 text-[10px] font-semibold ${opt.badgeColor}`}>
                            {opt.badge}
                          </span>
                          <p className="mt-2 text-[11px] leading-relaxed text-slate-400">{opt.desc}</p>
                          {est && (
                            <p className="mt-auto pt-2.5 text-xs font-bold text-emerald-400">
                              Est. Save: ~{formatBytes(est.estimated_saved_bytes)} (-{est.estimated_saved_pct.toFixed(0)}%)
                            </p>
                          )}
                        </button>
                      );
                    })}
                  </div>

                  {/* Extra Space-Saving Options */}
                  <div className="mt-4 space-y-3 border-t border-slate-800/80 pt-4">
                    <label className="flex cursor-pointer items-center justify-between rounded-xl bg-[#060911] px-4 py-3">
                      <div className="flex items-center gap-3">
                        <Copy className="h-4 w-4 text-amber-400" />
                        <div>
                          <p className="text-sm font-medium text-white">
                            Merge Duplicate Files Inside This Game (Safe)
                          </p>
                          <p className="text-xs text-slate-400">
                            Replaces identical copies inside this game with a single shared file. Zero FPS cost and safe when uninstalling.
                          </p>
                        </div>
                      </div>
                      <input
                        type="checkbox"
                        checked={enableHardlinks}
                        onChange={(e) => setEnableHardlinks(e.target.checked)}
                        className="h-4 w-4 accent-emerald-500"
                      />
                    </label>

                    {mode !== "hyper_archive" && (
                      <label
                        className={`flex items-center justify-between rounded-xl bg-[#060911] px-4 py-3 ${
                          scanPreview.anti_cheat_detected ? "opacity-50" : "cursor-pointer"
                        }`}
                      >
                        <div className="flex items-center gap-3">
                          <Archive className="h-4 w-4 text-cyan-400" />
                          <div>
                            <p className="text-sm font-medium text-white">
                              Archive Unused Foreign Voice Packs ({scanPreview.cold_vault_candidates} found •{" "}
                              {formatBytes(scanPreview.cold_vault_bytes)})
                            </p>
                            <p className="text-xs text-slate-400">
                              {scanPreview.anti_cheat_detected
                                ? "Disabled automatically because anti-cheat was detected."
                                : "Packs unused non-English voice files tightly with 1-click restore anytime."}
                            </p>
                          </div>
                        </div>
                        <input
                          type="checkbox"
                          disabled={scanPreview.anti_cheat_detected}
                          checked={enableColdVault}
                          onChange={(e) => setEnableColdVault(e.target.checked)}
                          className="h-4 w-4 accent-cyan-500"
                        />
                      </label>
                    )}
                  </div>

                  <button
                    onClick={handleCompress}
                    disabled={isRunning}
                    className={`mt-5 flex w-full items-center justify-center gap-2 rounded-xl py-3.5 text-sm font-bold text-slate-950 shadow-lg transition hover:brightness-110 disabled:opacity-50 ${
                      mode === "hyper_archive"
                        ? "bg-gradient-to-r from-indigo-400 to-cyan-400 shadow-indigo-500/20"
                        : "bg-gradient-to-r from-emerald-500 to-cyan-500 shadow-emerald-500/20"
                    }`}
                  >
                    {mode === "hyper_archive" ? <Snowflake className="h-4 w-4" /> : <Flame className="h-4 w-4" />}
                    {isRunning ? "Compressing..." : `Start ${formatModeLabel(mode)}`}
                  </button>
                </div>
              </div>
            )}
          </div>
        )}

        {/* TAB 3: GAME DETAILS & FILE HISTORY */}
        {tab === "inspector" && selectedDetail && (
          <div className="space-y-6">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-3">
                <button
                  onClick={() => setTab("dashboard")}
                  className="rounded-xl bg-slate-800/80 p-2 text-slate-300 hover:bg-slate-700"
                >
                  <ArrowLeft className="h-4 w-4" />
                </button>
                <div>
                  <h2 className="text-2xl font-bold text-white">{selectedDetail.game.name}</h2>
                  <p className="text-xs text-slate-400">{selectedDetail.game.path}</p>
                </div>
              </div>

              <div className="flex items-center gap-2.5">
                {selectedDetail.game.status !== "archived" && selectedDetail.game.exe_path && (
                  <button
                    onClick={() => handleLaunchExe(selectedDetail.game.exe_path)}
                    className="flex items-center gap-2 rounded-xl bg-emerald-500 px-4 py-2 text-sm font-semibold text-slate-950 hover:bg-emerald-400"
                  >
                    <Play className="h-4 w-4" />
                    Play Game
                  </button>
                )}
                <button
                  onClick={() => handleRestore(selectedDetail.game)}
                  disabled={isRunning}
                  className="flex items-center gap-2 rounded-xl bg-indigo-500/20 px-4 py-2 text-sm font-semibold text-indigo-200 hover:bg-indigo-500/30 disabled:opacity-40"
                >
                  <RotateCcw className="h-4 w-4" />
                  {selectedDetail.game.status === "archived" ? "Unpack to Play" : "Restore Original Files"}
                </button>
              </div>
            </div>

            <div className="grid grid-cols-5 gap-4">
              <div className="rounded-2xl border border-slate-800/80 bg-[#0b101b] p-4">
                <p className="text-xs text-slate-400">Before</p>
                <p className="mt-1 text-xl font-bold text-slate-200">
                  {formatBytes(selectedDetail.game.before_bytes)}
                </p>
              </div>
              <div className="rounded-2xl border border-slate-800/80 bg-[#0b101b] p-4">
                <p className="text-xs text-slate-400">After</p>
                <p className="mt-1 text-xl font-bold text-emerald-400">
                  {formatBytes(selectedDetail.game.after_bytes)}
                </p>
              </div>
              <div className="rounded-2xl border border-emerald-500/20 bg-emerald-500/5 p-4">
                <p className="text-xs text-emerald-400">Total Saved</p>
                <p className="mt-1 text-xl font-bold text-white">
                  {formatBytes(selectedDetail.game.saved_bytes)} ({selectedDetail.game.ratio_pct.toFixed(1)}%)
                </p>
              </div>
              <div className="rounded-2xl border border-slate-800/80 bg-[#0b101b] p-4">
                <p className="text-xs text-slate-400">Savings Breakdown</p>
                <p className="mt-1 text-xs font-semibold text-white">
                  Compressed: {formatBytes(selectedDetail.game.lzx_saved_bytes)}
                </p>
                <p className="text-xs text-amber-300">
                  Duplicates: {formatBytes(selectedDetail.game.dedup_saved_bytes)}
                </p>
                <p className="text-xs text-indigo-300">
                  Deep Freeze: {formatBytes(selectedDetail.game.vault_saved_bytes)}
                </p>
              </div>
              <div className="rounded-2xl border border-slate-800/80 bg-[#0b101b] p-4">
                <p className="text-xs text-slate-400">Time Taken</p>
                <p className="mt-1 text-sm font-semibold text-white">
                  {selectedDetail.game.duration_secs.toFixed(1)}s
                </p>
                <p className="text-[11px] text-slate-500">
                  {formatDate(selectedDetail.game.last_compressed_at)}
                </p>
              </div>
            </div>

            <div className="rounded-2xl border border-slate-800/80 bg-[#0b101b] p-5">
              <div className="mb-3 flex items-center gap-2">
                <Layers className="h-4 w-4 text-cyan-400" />
                <h3 className="text-sm font-semibold text-white">What Compressed (By File Type)</h3>
              </div>
              <div className="grid grid-cols-2 gap-3 md:grid-cols-3">
                {selectedDetail.categories.map((c) => {
                  const pct = c.before_bytes > 0 ? (c.saved_bytes / c.before_bytes) * 100 : 0;
                  return (
                    <div key={c.category} className="rounded-xl bg-[#060911] p-3.5">
                      <div className="flex items-center justify-between text-xs">
                        <span className="font-semibold text-slate-200">{c.category}</span>
                        <span className="font-bold text-emerald-400">-{pct.toFixed(1)}%</span>
                      </div>
                      <p className="mt-1 text-[11px] text-slate-400">
                        {formatBytes(c.before_bytes)} → {formatBytes(c.after_bytes)} ({c.file_count} files)
                      </p>
                      <div className="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-slate-800">
                        <div
                          className="h-full rounded-full bg-emerald-400"
                          style={{ width: `${Math.min(100, pct)}%` }}
                        />
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>

            <div className="rounded-2xl border border-slate-800/80 bg-[#0b101b] p-5">
              <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
                <h3 className="text-sm font-semibold text-white">
                  File-by-File Breakdown ({filteredAudits.length.toLocaleString()} files)
                </h3>
                <div className="flex items-center gap-2">
                  <div className="relative">
                    <Search className="absolute left-3 top-2.5 h-3.5 w-3.5 text-slate-500" />
                    <input
                      type="text"
                      value={auditSearch}
                      onChange={(e) => setAuditSearch(e.target.value)}
                      placeholder="Search files..."
                      className="rounded-lg border border-slate-700/80 bg-[#060911] py-1.5 pl-8 pr-3 text-xs text-slate-200 focus:border-emerald-500 focus:outline-none"
                    />
                  </div>
                  {[
                    { id: "ALL", label: "All" },
                    { id: "COMPRESSED", label: "Compressed" },
                    { id: "Duplicate Merged", label: "Duplicates" },
                    { id: "DEEP", label: "Deep Freeze" },
                    { id: "Already Compressed", label: "Skipped" },
                  ].map((f) => (
                    <button
                      key={f.id}
                      onClick={() => setAuditFilter(f.id)}
                      className={`rounded-lg px-2.5 py-1.5 text-[11px] font-medium transition ${
                        auditFilter === f.id
                          ? "bg-emerald-500/20 text-emerald-300 ring-1 ring-emerald-500/40"
                          : "bg-slate-800/70 text-slate-400 hover:text-slate-200"
                      }`}
                    >
                      {f.label}
                    </button>
                  ))}
                </div>
              </div>

              <div className="max-h-96 overflow-y-auto rounded-xl border border-slate-800/80">
                <table className="w-full text-left text-xs">
                  <thead className="sticky top-0 bg-[#0d1424] text-[11px] uppercase text-slate-400">
                    <tr>
                      <th className="px-3.5 py-2.5">File</th>
                      <th className="px-3.5 py-2.5">Method</th>
                      <th className="px-3.5 py-2.5 text-right">Before</th>
                      <th className="px-3.5 py-2.5 text-right">After</th>
                      <th className="px-3.5 py-2.5 text-right">Saved</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-slate-800/60">
                    {filteredAudits.slice(0, 300).map((a) => (
                      <tr key={a.id} className="hover:bg-slate-800/30">
                        <td
                          className="max-w-xs truncate px-3.5 py-2 font-mono text-[11px] text-slate-200"
                          title={a.rel_path}
                        >
                          {a.rel_path}
                          {a.hardlink_target && (
                            <span className="ml-2 text-[10px] text-amber-400">
                              → {a.hardlink_target}
                            </span>
                          )}
                        </td>
                        <td className="px-3 py-2">
                          <span
                            className={`rounded px-2 py-0.5 text-[10px] font-semibold ${
                              a.action === "Max Compressed" || a.action === "Fast Compressed"
                                ? "bg-emerald-500/15 text-emerald-300"
                                : a.action === "Duplicate Merged"
                                  ? "bg-amber-500/15 text-amber-300"
                                  : a.action.startsWith("Deep Freeze") || a.action === "Archived Language"
                                    ? "bg-indigo-500/20 text-indigo-300"
                                    : "bg-slate-800 text-slate-400"
                            }`}
                          >
                            {a.action}
                          </span>
                        </td>
                        <td className="px-3 py-2 text-right text-slate-400">{formatBytes(a.before_bytes)}</td>
                        <td className="px-3 py-2 text-right text-slate-200">{formatBytes(a.after_bytes)}</td>
                        <td className="px-3 py-2 text-right font-semibold text-emerald-400">
                          {a.saved_bytes > 0 ? `-${formatBytes(a.saved_bytes)}` : "—"}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          </div>
        )}
      </main>
    </div>
  );
}
