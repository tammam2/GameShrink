import React, { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Archive,
  ArrowLeft,
  CheckCircle2,
  Cpu,
  Database,
  FileSearch,
  Flame,
  FlaskConical,
  FolderOpen,
  HardDrive,
  Layers,
  Link2,
  Play,
  RotateCcw,
  Search,
  ShieldAlert,
  ShieldCheck,
  Snowflake,
  Sparkles,
  Trash2,
  Zap,
} from "lucide-react";
import {
  BenchmarkReport,
  CompressionMode,
  GameDetailResponse,
  GameRecord,
  GlobalStats,
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

  // Scanner & Compression state
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
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [toastMsg, setToastMsg] = useState<string | null>(null);

  // Inspector filters
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
        title: "Select Game Installation Folder",
      });
      if (typeof selected === "string" && selected.length > 0) {
        setFolderPath(selected);
        await runXRayScan(selected);
      }
    } catch (e) {
      setErrorMsg(String(e));
    }
  };

  const runXRayScan = async (targetPath: string) => {
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

  const handleRunBenchmark = async () => {
    if (!folderPath.trim()) return;
    setIsBenchRunning(true);
    setErrorMsg(null);
    try {
      const report = await invoke<BenchmarkReport>("benchmark_beyond_sota", {
        path: folderPath.trim(),
      });
      setBenchmark(report);
    } catch (e) {
      setErrorMsg(String(e));
    } finally {
      setIsBenchRunning(false);
    }
  };

  const handleCompress = async () => {
    if (!folderPath.trim()) return;
    setIsRunning(true);
    setErrorMsg(null);
    setProgress({
      phase: "Initializing Engine",
      current_file: "Preparing worker threads...",
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
      showToast(
        `Crushed ${record.name} — Reclaimed ${formatBytes(record.saved_bytes)} (${record.ratio_pct.toFixed(1)}%)`
      );
    } catch (e) {
      setErrorMsg(String(e));
    } finally {
      setIsRunning(false);
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
    setErrorMsg(null);
    try {
      await invoke<GameRecord>("restore_game", {
        gameId: game.id,
        breakHardlinks: true,
      });
      await refreshLibrary();
      if (selectedDetail?.game.id === game.id) {
        await openInspector(game.id);
      }
      showToast(`Hydrated & restored ${game.name} bit-for-bit losslessly.`);
    } catch (e) {
      setErrorMsg(String(e));
    } finally {
      setIsRunning(false);
    }
  };

  const handleLaunchExe = async (exePath: string | null) => {
    if (!exePath) {
      setErrorMsg("No game executable was auto-detected for this folder.");
      return;
    }
    try {
      await invoke("launch_game_exe", { exePath });
      showToast("Launched game directly from its .exe (zero wrapper overhead).");
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
      showToast("Removed game from ledger (files on disk untouched).");
    } catch (e) {
      setErrorMsg(String(e));
    }
  };

  const filteredAudits = useMemo(() => {
    if (!selectedDetail) return [];
    const q = auditSearch.trim().toLowerCase();
    return selectedDetail.audits.filter((a) => {
      const matchAction =
        auditFilter === "ALL" ||
        (auditFilter === "HYPER"
          ? a.action.includes("ChronoDelta") ||
            a.action.includes("CanonPack") ||
            a.action.includes("AutoSIMD") ||
            a.action.includes("Zstd")
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
            <h1 className="text-base font-bold tracking-tight text-white">AetherCrush v2</h1>
            <p className="text-[11px] font-medium text-emerald-400">Beyond-SOTA Engine</p>
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
            Library Dashboard
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
            Compress & Lab
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
              <Database className="h-4 w-4" />
              Ledger Inspector
            </button>
          )}
        </nav>

        {/* Total Reclaimed Hero Widget */}
        <div className="mt-auto rounded-2xl border border-emerald-500/20 bg-gradient-to-b from-emerald-500/10 to-slate-900/60 p-4">
          <p className="text-[11px] font-semibold uppercase tracking-wider text-emerald-400">
            Total Space Reclaimed
          </p>
          <p className="mt-1 text-2xl font-bold text-white">
            {formatBytes(stats?.total_saved_bytes ?? 0)}
          </p>
          <p className="mt-1 text-xs text-slate-400">
            Across {stats?.total_games ?? 0} game{(stats?.total_games ?? 0) === 1 ? "" : "s"}
          </p>
          <div className="mt-3 space-y-1 text-[11px] text-slate-400">
            <div className="flex justify-between">
              <span>NTFS LZX:</span>
              <span className="text-slate-200">{formatBytes(stats?.total_lzx_saved_bytes ?? 0)}</span>
            </div>
            <div className="flex justify-between">
              <span>Hardlinks:</span>
              <span className="text-amber-300">{formatBytes(stats?.total_dedup_saved_bytes ?? 0)}</span>
            </div>
            <div className="flex justify-between">
              <span>HyperVault:</span>
              <span className="text-cyan-300">{formatBytes(stats?.total_vault_saved_bytes ?? 0)}</span>
            </div>
          </div>
        </div>
      </aside>

      {/* Main Content */}
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

        {isRunning && progress && (
          <div className="mb-6 rounded-2xl border border-emerald-500/30 bg-[#0c1322] p-5 shadow-xl shadow-emerald-950/30">
            <div className="flex items-center justify-between">
              <div>
                <p className="text-xs font-semibold uppercase tracking-wider text-emerald-400">
                  {progress.phase}
                </p>
                <p className="mt-1 max-w-xl truncate text-xs text-slate-400">
                  {progress.current_file}
                </p>
              </div>
              <div className="text-right">
                <p className="text-lg font-bold text-white">{progress.pct.toFixed(1)}%</p>
                <p className="text-xs text-emerald-400">
                  Saved so far: {formatBytes(progress.saved_so_far_bytes)}
                </p>
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

        {/* TAB 1: DASHBOARD */}
        {tab === "dashboard" && (
          <div className="space-y-6">
            <div className="flex items-center justify-between">
              <div>
                <h2 className="text-2xl font-bold text-white">Game Storage Telemetry</h2>
                <p className="mt-1 text-sm text-slate-400">
                  Playable NTFS LZX + Intra-Game Hardlinks + Beyond-SOTA HyperVault (ChronoDelta, CanonPack & Auto-SIMD).
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
                <p className="text-xs text-slate-400">Total Saved</p>
                <p className="mt-1.5 text-2xl font-bold text-emerald-400">
                  {formatBytes(stats?.total_saved_bytes ?? 0)}
                </p>
                <p className="mt-1 text-[11px] text-slate-500">
                  {formatBytes(stats?.total_before_bytes ?? 0)} → {formatBytes(stats?.total_after_bytes ?? 0)}
                </p>
              </div>

              <div className="rounded-2xl border border-slate-800/80 bg-[#0b101b] p-4">
                <p className="text-xs text-slate-400">Average Crush Ratio</p>
                <p className="mt-1.5 text-2xl font-bold text-cyan-400">
                  {(stats?.avg_ratio_pct ?? 0).toFixed(1)}%
                </p>
                <p className="mt-1 text-[11px] text-slate-500">Physical bytes reclaimed</p>
              </div>

              <div className="rounded-2xl border border-slate-800/80 bg-[#0b101b] p-4">
                <p className="text-xs text-slate-400">HyperVault Savings</p>
                <p className="mt-1.5 text-2xl font-bold text-indigo-300">
                  {formatBytes(stats?.total_vault_saved_bytes ?? 0)}
                </p>
                <p className="mt-1 text-[11px] text-slate-500">ChronoDelta + CanonPack + Auto-SIMD</p>
              </div>

              <div className="rounded-2xl border border-slate-800/80 bg-[#0b101b] p-4">
                <p className="text-xs text-slate-400">Duplicates Hardlinked</p>
                <p className="mt-1.5 text-2xl font-bold text-amber-300">
                  {(stats?.total_files_hardlinked ?? 0).toLocaleString()}
                </p>
                <p className="mt-1 text-[11px] text-slate-500">
                  {formatBytes(stats?.total_dedup_saved_bytes ?? 0)} freed intra-game
                </p>
              </div>
            </div>

            {games.length === 0 ? (
              <div className="flex flex-col items-center justify-center rounded-2xl border border-dashed border-slate-800 bg-[#0b101b]/50 py-16 text-center">
                <HardDrive className="mb-3 h-10 w-10 text-slate-600" />
                <h3 className="text-base font-semibold text-slate-200">No games in your ledger yet</h3>
                <p className="mt-1 max-w-md text-sm text-slate-400">
                  Scan any game folder to benchmark Beyond-SOTA codecs live, or crush it immediately with LZX + Hardlinks or HyperArchive.
                </p>
                <button
                  onClick={() => setTab("compress")}
                  className="mt-5 rounded-xl bg-emerald-500/15 px-4 py-2 text-sm font-medium text-emerald-300 ring-1 ring-emerald-500/30 hover:bg-emerald-500/25"
                >
                  Open X-Ray Scanner & Codec Lab
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
                            {g.anti_cheat_detected && (
                              <span
                                title={`Detected: ${g.anti_cheat_name}`}
                                className="flex items-center gap-1 rounded-md bg-amber-500/15 px-2 py-0.5 text-[10px] font-semibold text-amber-300"
                              >
                                <ShieldCheck className="h-3 w-3" />
                                AC-Safe
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

                      <div className="mt-3 flex flex-wrap gap-3 text-[11px] text-slate-400">
                        {g.lzx_saved_bytes > 0 && <span>LZX: {formatBytes(g.lzx_saved_bytes)}</span>}
                        {g.files_hardlinked > 0 && (
                          <span>Hardlinks: {g.files_hardlinked} ({formatBytes(g.dedup_saved_bytes)})</span>
                        )}
                        {g.files_vaulted > 0 && (
                          <span className="text-indigo-300">
                            HyperVault: {g.files_vaulted} ({formatBytes(g.vault_saved_bytes)})
                          </span>
                        )}
                      </div>

                      <div className="mt-4 flex items-center gap-2 border-t border-slate-800/80 pt-3.5">
                        {isArchived ? (
                          <button
                            onClick={() => handleRestore(g)}
                            disabled={isRunning}
                            className="flex items-center gap-1.5 rounded-lg bg-indigo-500/20 px-3 py-1.5 text-xs font-semibold text-indigo-200 hover:bg-indigo-500/30 disabled:opacity-40"
                          >
                            <RotateCcw className="h-3.5 w-3.5" />
                            Hydrate to Play
                          </button>
                        ) : (
                          g.exe_path && (
                            <button
                              onClick={() => handleLaunchExe(g.exe_path)}
                              className="flex items-center gap-1.5 rounded-lg bg-emerald-500/15 px-3 py-1.5 text-xs font-semibold text-emerald-300 hover:bg-emerald-500/25"
                            >
                              <Play className="h-3.5 w-3.5" />
                              Play .exe
                            </button>
                          )
                        )}
                        <button
                          onClick={() => openInspector(g.id)}
                          className="flex items-center gap-1.5 rounded-lg bg-slate-800 px-3 py-1.5 text-xs font-medium text-slate-200 hover:bg-slate-700"
                        >
                          <Database className="h-3.5 w-3.5" />
                          Inspect Ledger
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
                          title="Remove from ledger"
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

        {/* TAB 2: COMPRESS & BEYOND-SOTA CODEC LAB */}
        {tab === "compress" && (
          <div className="mx-auto max-w-5xl space-y-6">
            <div>
              <h2 className="text-2xl font-bold text-white">X-Ray Scanner & Beyond-SOTA Engine</h2>
              <p className="mt-1 text-sm text-slate-400">
                Scan Shannon entropy, exact duplicates, and ChronoDelta near-duplicate clusters—or run a live non-destructive codec benchmark.
              </p>
            </div>

            <div className="rounded-2xl border border-slate-800/80 bg-[#0b101b] p-5">
              <label className="text-xs font-semibold uppercase tracking-wider text-slate-400">
                Game Installation Folder
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
                  onClick={() => runXRayScan(folderPath)}
                  disabled={isScanning || !folderPath.trim()}
                  className="flex items-center gap-2 rounded-xl bg-cyan-500/15 px-4 py-2.5 text-sm font-semibold text-cyan-300 ring-1 ring-cyan-500/30 hover:bg-cyan-500/25 disabled:opacity-40"
                >
                  <FileSearch className="h-4 w-4" />
                  {isScanning ? "Scanning..." : "X-Ray Scan"}
                </button>
              </div>
            </div>

            {scanPreview && (
              <div className="space-y-5">
                {/* X-Ray Summary */}
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
                        {scanPreview.total_files.toLocaleString()} files • Main exe:{" "}
                        <span className="text-slate-200">
                          {scanPreview.detected_exe ? scanPreview.detected_exe.split(/[\\/]/).pop() : "Not auto-detected"}
                        </span>
                      </p>
                    </div>

                    <div className="flex items-center gap-2.5">
                      <button
                        onClick={handleRunBenchmark}
                        disabled={isBenchRunning}
                        className="flex items-center gap-2 rounded-xl bg-indigo-500/15 px-3.5 py-2 text-xs font-semibold text-indigo-300 ring-1 ring-indigo-500/30 hover:bg-indigo-500/25 disabled:opacity-40"
                      >
                        <FlaskConical className="h-4 w-4" />
                        {isBenchRunning ? "Benchmarking..." : "Run Beyond-SOTA Benchmark"}
                      </button>

                      {scanPreview.anti_cheat_detected ? (
                        <div className="flex items-center gap-2 rounded-xl border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-200">
                          <ShieldAlert className="h-4 w-4 text-amber-400" />
                          <span>{scanPreview.anti_cheat_name}</span>
                        </div>
                      ) : (
                        <div className="flex items-center gap-2 rounded-xl border border-emerald-500/30 bg-emerald-500/10 px-3 py-2 text-xs text-emerald-200">
                          <ShieldCheck className="h-4 w-4 text-emerald-400" />
                          <span>No Anti-Cheat</span>
                        </div>
                      )}
                    </div>
                  </div>

                  <div className="mt-5 grid grid-cols-5 gap-3">
                    <div className="rounded-xl bg-[#060911] p-3.5">
                      <p className="text-[11px] text-slate-400">Current Disk Size</p>
                      <p className="mt-1 text-lg font-bold text-white">
                        {formatBytes(scanPreview.current_disk_bytes)}
                      </p>
                    </div>
                    <div className="rounded-xl bg-[#060911] p-3.5">
                      <p className="text-[11px] text-slate-400">Est. LZX Savings</p>
                      <p className="mt-1 text-lg font-bold text-emerald-400">
                        {formatBytes(scanPreview.estimated_saved_bytes)}
                      </p>
                    </div>
                    <div className="rounded-xl bg-[#060911] p-3.5">
                      <p className="text-[11px] text-slate-400">Exact Duplicates</p>
                      <p className="mt-1 text-lg font-bold text-amber-300">
                        {scanPreview.duplicate_files} ({formatBytes(scanPreview.duplicate_reclaimable_bytes)})
                      </p>
                    </div>
                    <div className="rounded-xl bg-[#060911] p-3.5">
                      <p className="text-[11px] text-slate-400">ChronoDelta Pairs</p>
                      <p className="mt-1 text-lg font-bold text-indigo-300">
                        {scanPreview.chrono_delta_pairs} near-dupes
                      </p>
                    </div>
                    <div className="rounded-xl bg-[#060911] p-3.5">
                      <p className="text-[11px] text-slate-400">High-Entropy (Oodle)</p>
                      <p className="mt-1 text-lg font-bold text-slate-300">
                        {formatBytes(scanPreview.high_entropy_bytes)}
                      </p>
                    </div>
                  </div>
                </div>

                {/* Live Beyond-SOTA Benchmark Results */}
                {benchmark && (
                  <div className="rounded-2xl border border-indigo-500/30 bg-[#0b1122] p-5">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <FlaskConical className="h-4 w-4 text-indigo-400" />
                        <h3 className="text-sm font-bold text-white">
                          Live Beyond-SOTA Codec Lab ({benchmark.sample_count} real sample files)
                        </h3>
                      </div>
                      <span className="rounded-lg bg-emerald-500/15 px-2.5 py-1 text-xs font-semibold text-emerald-300">
                        Extra saved beyond Zstd-19: {formatBytes(benchmark.extra_saved_over_zstd_bytes)}
                      </span>
                    </div>
                    <p className="mt-1 text-xs text-slate-400">
                      Sample Raw: {formatBytes(benchmark.total_raw_bytes)} → Zstd-19:{" "}
                      {formatBytes(benchmark.total_zstd19_bytes)} →{" "}
                      <strong className="text-indigo-300">
                        HyperCodec (ChronoDelta + CanonPack + Auto-SIMD): {formatBytes(benchmark.total_hyper_bytes)}
                      </strong>
                    </p>
                    <div className="mt-3 max-h-52 overflow-y-auto rounded-xl border border-slate-800">
                      <table className="w-full text-left text-xs">
                        <thead className="bg-[#080d1a] text-[10px] uppercase text-slate-400">
                          <tr>
                            <th className="px-3 py-2">Sample File</th>
                            <th className="px-3 py-2">Winning Pipeline</th>
                            <th className="px-3 py-2 text-right">Raw</th>
                            <th className="px-3 py-2 text-right">Zstd-19</th>
                            <th className="px-3 py-2 text-right">HyperCodec</th>
                            <th className="px-3 py-2 text-right">Beat Zstd-19</th>
                          </tr>
                        </thead>
                        <tbody className="divide-y divide-slate-800/60">
                          {benchmark.rows.map((r) => (
                            <tr key={r.rel_path}>
                              <td className="max-w-[200px] truncate px-3 py-1.5 font-mono text-[11px] text-slate-200">
                                {r.rel_path}
                              </td>
                              <td className="px-3 py-1.5 text-[11px] font-semibold text-indigo-300">
                                {r.winning_codec}
                              </td>
                              <td className="px-3 py-1.5 text-right text-slate-400">{formatBytes(r.raw_bytes)}</td>
                              <td className="px-3 py-1.5 text-right text-slate-300">{formatBytes(r.zstd19_bytes)}</td>
                              <td className="px-3 py-1.5 text-right font-semibold text-emerald-400">
                                {formatBytes(r.hyper_bytes)}
                              </td>
                              <td className="px-3 py-1.5 text-right text-cyan-300">
                                {r.extra_gain_pct > 0 ? `+${r.extra_gain_pct.toFixed(1)}%` : "0.0%"}
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  </div>
                )}

                {/* 4-Mode Compression Selector */}
                <div className="rounded-2xl border border-slate-800/80 bg-[#0b101b] p-5">
                  <h3 className="text-sm font-semibold text-white">Compression Engine Profile</h3>
                  <div className="mt-3 grid grid-cols-2 gap-3 lg:grid-cols-4">
                    <button
                      onClick={() => setMode("max_crush")}
                      className={`rounded-xl border p-4 text-left transition ${
                        mode === "max_crush"
                          ? "border-emerald-500 bg-emerald-500/10"
                          : "border-slate-800 bg-[#060911] hover:border-slate-700"
                      }`}
                    >
                      <div className="flex items-center justify-between">
                        <span className="text-xs font-bold text-white">Max Crush (LZX)</span>
                        <Flame className="h-4 w-4 text-emerald-400" />
                      </div>
                      <span className="mt-1 inline-block rounded bg-emerald-500/15 px-1.5 py-0.5 text-[10px] font-semibold text-emerald-300">
                        Direct .exe Play
                      </span>
                      <p className="mt-1.5 text-[11px] text-slate-400">
                        Strongest native NTFS <strong>LZX</strong> + Shannon entropy guard. Zero launcher needed.
                      </p>
                    </button>

                    <button
                      onClick={() => setMode("brute_max")}
                      className={`rounded-xl border p-4 text-left transition ${
                        mode === "brute_max"
                          ? "border-cyan-500 bg-cyan-500/10"
                          : "border-slate-800 bg-[#060911] hover:border-slate-700"
                      }`}
                    >
                      <div className="flex items-center justify-between">
                        <span className="text-xs font-bold text-white">Brute Max (All LZX)</span>
                        <Sparkles className="h-4 w-4 text-cyan-400" />
                      </div>
                      <span className="mt-1 inline-block rounded bg-cyan-500/15 px-1.5 py-0.5 text-[10px] font-semibold text-cyan-300">
                        Direct .exe Play
                      </span>
                      <p className="mt-1.5 text-[11px] text-slate-400">
                        Forces <strong>LZX</strong> on every single file regardless of entropy to squeeze every MB.
                      </p>
                    </button>

                    <button
                      onClick={() => setMode("adaptive_thermal")}
                      className={`rounded-xl border p-4 text-left transition ${
                        mode === "adaptive_thermal"
                          ? "border-purple-500 bg-purple-500/10"
                          : "border-slate-800 bg-[#060911] hover:border-slate-700"
                      }`}
                    >
                      <div className="flex items-center justify-between">
                        <span className="text-xs font-bold text-white">Adaptive Thermal</span>
                        <Cpu className="h-4 w-4 text-purple-400" />
                      </div>
                      <span className="mt-1 inline-block rounded bg-purple-500/15 px-1.5 py-0.5 text-[10px] font-semibold text-purple-300">
                        Direct .exe Play
                      </span>
                      <p className="mt-1.5 text-[11px] text-slate-400">
                        <strong>LZX</strong> on static/startup files + <strong>XPRESS8K</strong> on open-world streams.
                      </p>
                    </button>

                    <button
                      onClick={() => setMode("hyper_archive")}
                      className={`rounded-xl border p-4 text-left transition ${
                        mode === "hyper_archive"
                          ? "border-indigo-400 bg-indigo-500/15"
                          : "border-slate-800 bg-[#060911] hover:border-slate-700"
                      }`}
                    >
                      <div className="flex items-center justify-between">
                        <span className="text-xs font-bold text-white">HyperArchive</span>
                        <Snowflake className="h-4 w-4 text-indigo-300" />
                      </div>
                      <span className="mt-1 inline-block rounded bg-indigo-500/20 px-1.5 py-0.5 text-[10px] font-semibold text-indigo-300">
                        Beyond-SOTA Max
                      </span>
                      <p className="mt-1.5 text-[11px] text-slate-400">
                        Stacks <strong>ChronoDelta + CanonPack + Auto-SIMD + Zstd-19</strong>. 1-click Hydrate to play.
                      </p>
                    </button>
                  </div>

                  {/* Toggles */}
                  <div className="mt-4 space-y-3 border-t border-slate-800/80 pt-4">
                    <label className="flex cursor-pointer items-center justify-between rounded-xl bg-[#060911] px-4 py-3">
                      <div className="flex items-center gap-3">
                        <Link2 className="h-4 w-4 text-amber-400" />
                        <div>
                          <p className="text-sm font-medium text-white">
                            Intra-Game Hardlink Deduplication (Single-Game Scope)
                          </p>
                          <p className="text-xs text-slate-400">
                            Collapses exact duplicate static files inside this game into 1 physical copy (0% FPS cost).
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
                              HyperVault Cold Foreign Voice Packs ({scanPreview.cold_vault_candidates} found •{" "}
                              {formatBytes(scanPreview.cold_vault_bytes)})
                            </p>
                            <p className="text-xs text-slate-400">
                              {scanPreview.anti_cheat_detected
                                ? "Locked OFF because kernel anti-cheat was detected."
                                : "Crushes unused foreign voice packs with Auto-SIMD + Zstd-19 while keeping the game directly playable."}
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
                    {isRunning ? "Running Engine..." : `Start ${formatModeLabel(mode)} & Save Ledger`}
                  </button>
                </div>
              </div>
            )}
          </div>
        )}

        {/* TAB 3: GAME LEDGER & FILE INSPECTOR */}
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
                    Play .exe Directly
                  </button>
                )}
                <button
                  onClick={() => handleRestore(selectedDetail.game)}
                  disabled={isRunning}
                  className="flex items-center gap-2 rounded-xl bg-indigo-500/20 px-4 py-2 text-sm font-semibold text-indigo-200 hover:bg-indigo-500/30 disabled:opacity-40"
                >
                  <RotateCcw className="h-4 w-4" />
                  {selectedDetail.game.status === "archived" ? "Hydrate Game to Play" : "1-Click Restore"}
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
                <p className="text-xs text-emerald-400">Total Reclaimed</p>
                <p className="mt-1 text-xl font-bold text-white">
                  {formatBytes(selectedDetail.game.saved_bytes)} ({selectedDetail.game.ratio_pct.toFixed(1)}%)
                </p>
              </div>
              <div className="rounded-2xl border border-slate-800/80 bg-[#0b101b] p-4">
                <p className="text-xs text-slate-400">Method Split</p>
                <p className="mt-1 text-xs font-semibold text-white">
                  LZX: {formatBytes(selectedDetail.game.lzx_saved_bytes)}
                </p>
                <p className="text-xs text-amber-300">
                  Dedup: {formatBytes(selectedDetail.game.dedup_saved_bytes)}
                </p>
                <p className="text-xs text-indigo-300">
                  HyperVault: {formatBytes(selectedDetail.game.vault_saved_bytes)}
                </p>
              </div>
              <div className="rounded-2xl border border-slate-800/80 bg-[#0b101b] p-4">
                <p className="text-xs text-slate-400">Last Run</p>
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
                <h3 className="text-sm font-semibold text-white">What Compressed (By Category)</h3>
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
                  Per-File Compression Ledger ({filteredAudits.length.toLocaleString()} shown)
                </h3>
                <div className="flex items-center gap-2">
                  <div className="relative">
                    <Search className="absolute left-3 top-2.5 h-3.5 w-3.5 text-slate-500" />
                    <input
                      type="text"
                      value={auditSearch}
                      onChange={(e) => setAuditSearch(e.target.value)}
                      placeholder="Filter files or codecs..."
                      className="rounded-lg border border-slate-700/80 bg-[#060911] py-1.5 pl-8 pr-3 text-xs text-slate-200 focus:border-emerald-500 focus:outline-none"
                    />
                  </div>
                  {["ALL", "LZX", "Hardlinked", "HYPER", "Skipped-HighEntropy"].map((f) => (
                    <button
                      key={f}
                      onClick={() => setAuditFilter(f)}
                      className={`rounded-lg px-2.5 py-1.5 text-[11px] font-medium transition ${
                        auditFilter === f
                          ? "bg-emerald-500/20 text-emerald-300 ring-1 ring-emerald-500/40"
                          : "bg-slate-800/70 text-slate-400 hover:text-slate-200"
                      }`}
                    >
                      {f === "Skipped-HighEntropy" ? "Skipped" : f === "HYPER" ? "Beyond-SOTA" : f}
                    </button>
                  ))}
                </div>
              </div>

              <div className="max-h-96 overflow-y-auto rounded-xl border border-slate-800/80">
                <table className="w-full text-left text-xs">
                  <thead className="sticky top-0 bg-[#0d1424] text-[11px] uppercase text-slate-400">
                    <tr>
                      <th className="px-3.5 py-2.5">File Path</th>
                      <th className="px-3.5 py-2.5">Codec / Action</th>
                      <th className="px-3.5 py-2.5 text-right">Before</th>
                      <th className="px-3.5 py-2.5 text-right">After</th>
                      <th className="px-3.5 py-2.5 text-right">Saved</th>
                      <th className="px-3.5 py-2.5 text-right">Entropy</th>
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
                              a.action === "LZX"
                                ? "bg-emerald-500/15 text-emerald-300"
                                : a.action === "Hardlinked"
                                  ? "bg-amber-500/15 text-amber-300"
                                  : a.action.includes("ChronoDelta")
                                    ? "bg-indigo-500/20 text-indigo-300"
                                    : a.action.includes("CanonPack") || a.action.includes("AutoSIMD")
                                      ? "bg-cyan-500/15 text-cyan-300"
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
                        <td className="px-3 py-2 text-right text-slate-400">{a.entropy.toFixed(2)}</td>
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
