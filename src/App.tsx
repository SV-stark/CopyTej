import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

// Interface definitions
interface TransferFile {
  src: string;
  dest: string;
  bytes_total: number;
  bytes_done: number;
  status: "Queued" | "Copying" | "Verifying" | "Done" | "Skipped" | { Error: string };
  hash_src?: string;
  hash_dest?: string;
  error?: string;
}

interface TransferJob {
  id: string;
  src_paths: string[];
  dest_dir: string;
  is_move: boolean;
  status: "Queued" | "Running" | "Paused" | "Done" | { Error: string };
  files: TransferFile[];
  bytes_total: number;
  bytes_done: number;
  speed_bps: number;
  started_at?: number;
  finished_at?: number;
}

interface ConflictInfo {
  conflict_id: string;
  job_id: string;
  file_path: string;
  src_size: number;
  src_modified: number;
  dest_size: number;
  dest_modified: number;
}

interface ActiveQueueSidebarProps {
  activeJobs: TransferJob[];
  selectedJob: TransferJob | null;
  setSelectedJob: (job: TransferJob) => void;
  getPercent: (job: TransferJob) => number;
  formatSpeed: (bps: number) => string;
  getStatusString: (status: any) => string;
}

function ActiveQueueSidebar({
  activeJobs,
  selectedJob,
  setSelectedJob,
  getPercent,
  formatSpeed,
  getStatusString,
}: ActiveQueueSidebarProps) {
  return (
    <div className="right-panel">
      <section className="glass-card" style={{ flexGrow: 1, display: "flex", flexDirection: "column" }}>
        <h3 className="card-title" style={{ marginBottom: "16px" }}>Active Queue</h3>
        <div className="job-list">
          {activeJobs.length === 0 ? (
            <div className="empty-state" style={{ padding: "32px 0" }}>
              <p style={{ fontSize: "0.9rem" }}>No queued jobs.</p>
            </div>
          ) : (
            activeJobs.map(job => (
              <div
                className={`job-card-small ${selectedJob?.id === job.id ? "active" : ""}`}
                key={job.id}
                onClick={() => setSelectedJob(job)}
              >
                <div className="job-card-header">
                  <span className="job-card-title">{job.dest_dir.split(/[\\/]/).pop()}</span>
                  <span className={`badge badge-${(typeof job.status === "object" ? "error" : job.status).toLowerCase()}`}>
                    {getStatusString(job.status)}
                  </span>
                </div>
                <div className="progress-track" style={{ height: "4px", margin: "8px 0" }}>
                  <div className="progress-bar-fill" style={{ width: `${getPercent(job)}%` }}></div>
                </div>
                <div className="job-card-meta">
                  <span>{getPercent(job)}%</span>
                  <span>{formatSpeed(job.speed_bps)}</span>
                </div>
              </div>
            ))
          )}
        </div>
      </section>
    </div>
  );
}

function App() {
  const [activeTab, setActiveTab] = useState<"dashboard" | "topology" | "history" | "settings">("dashboard");
  
  // Data States
  const [activeJobs, setActiveJobs] = useState<TransferJob[]>([]);
  const [selectedJob, setSelectedJob] = useState<TransferJob | null>(null);
  const [historyJobs, setHistoryJobs] = useState<TransferJob[]>([]);
  const [historyPage, setHistoryPage] = useState(0);
  const historyPerPage = 10;
  const [conflict, setConflict] = useState<ConflictInfo | null>(null);
  
  // Modal / Input States
  const [newJobModal, setNewJobModal] = useState(false);
  const [srcPaths, setSrcPaths] = useState<string[]>([]);
  const [destDir, setDestDir] = useState("");
  const [isMove, setIsMove] = useState(false);
  const [newJobError, setNewJobError] = useState("");

  // Settings states
  const [autoVerify, setAutoVerify] = useState(true);
  const [hashAlgorithm, setHashAlgorithm] = useState("Blake3");
  const [enableBlockCloning, setEnableBlockCloning] = useState(true);
  const [speedLimitKbps, setSpeedLimitKbps] = useState(0);
  const [explorerContextMenu, setExplorerContextMenu] = useState(false);
  const [enableSounds, setEnableSounds] = useState(true);

  // Keep a ref for the selectedJob to use inside event listeners (to prevent stale closures)
  const selectedJobRef = useRef<TransferJob | null>(null);
  selectedJobRef.current = selectedJob;

  const enableSoundsRef = useRef(enableSounds);
  enableSoundsRef.current = enableSounds;

  const playChime = (type: 'success' | 'error') => {
    if (!enableSoundsRef.current) return;
    try {
      const audioCtx = new (window.AudioContext || (window as any).webkitAudioContext)();
      if (type === 'success') {
        const notes = [523.25, 659.25, 783.99, 1046.50];
        notes.forEach((freq, index) => {
          const osc = audioCtx.createOscillator();
          const gainNode = audioCtx.createGain();
          osc.type = 'sine';
          osc.frequency.setValueAtTime(freq, audioCtx.currentTime + index * 0.1);
          gainNode.gain.setValueAtTime(0.15, audioCtx.currentTime + index * 0.1);
          gainNode.gain.exponentialRampToValueAtTime(0.001, audioCtx.currentTime + index * 0.1 + 0.4);
          osc.connect(gainNode);
          gainNode.connect(audioCtx.destination);
          osc.start(audioCtx.currentTime + index * 0.1);
          osc.stop(audioCtx.currentTime + index * 0.1 + 0.4);
        });
      } else {
        const notes = [622.25, 523.25];
        notes.forEach((freq, index) => {
          const osc = audioCtx.createOscillator();
          const gainNode = audioCtx.createGain();
          osc.type = 'triangle';
          osc.frequency.setValueAtTime(freq, audioCtx.currentTime + index * 0.15);
          gainNode.gain.setValueAtTime(0.2, audioCtx.currentTime + index * 0.15);
          gainNode.gain.exponentialRampToValueAtTime(0.001, audioCtx.currentTime + index * 0.15 + 0.5);
          osc.connect(gainNode);
          gainNode.connect(audioCtx.destination);
          osc.start(audioCtx.currentTime + index * 0.15);
          osc.stop(audioCtx.currentTime + index * 0.15 + 0.5);
        });
      }
    } catch (e) {
      console.error("Web Audio API not supported:", e);
    }
  };

  // Refresh active jobs helper
  const refreshActiveJobs = async () => {
    try {
      const jobs = await invoke<TransferJob[]>("get_active_jobs");
      setActiveJobs(jobs);
      
      // Update selected job details if one is selected
      if (selectedJobRef.current) {
        const found = jobs.find(j => j.id === selectedJobRef.current?.id);
        if (found) {
          setSelectedJob(found);
        } else {
          // If not in active, it might have finished. Fetch its details from history
          const finishedJob = await invoke<TransferJob | null>("get_job_details", { jobId: selectedJobRef.current.id });
          if (finishedJob) {
            setSelectedJob(finishedJob);
          }
        }
      } else if (jobs.length > 0 && !selectedJob) {
        setSelectedJob(jobs[0]);
      }
    } catch (e) {
      console.error("Failed to fetch active jobs:", e);
    }
  };

  // Refresh history
  const refreshHistory = async (page = historyPage) => {
    try {
      const history = await invoke<TransferJob[]>("get_history", {
        limit: historyPerPage,
        offset: page * historyPerPage,
      });
      setHistoryJobs(history);
    } catch (e) {
      console.error("Failed to fetch history:", e);
    }
  };
  const handleClearHistory = async () => {
    if (confirm("Are you sure you want to clear completed transfer history?")) {
      try {
        await invoke("clear_history");
        refreshHistory(0);
        setHistoryPage(0);
      } catch (e) {
        console.error("Failed to clear history:", e);
      }
    }
  };

  const handleDeleteHistoryJob = async (jobId: string, e: React.MouseEvent) => {
    e.stopPropagation();
    if (confirm("Are you sure you want to delete this job history?")) {
      try {
        await invoke("delete_job", { jobId });
        refreshHistory(historyPage);
      } catch (e) {
        console.error("Failed to delete job history:", e);
      }
    }
  };

  const handleExportJobReport = async (jobId: string, format: 'csv' | 'json', e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      const res = await invoke<string>("export_job_report", { jobId, format });
      alert(res);
    } catch (e) {
      if (e !== "Cancelled") {
        alert("Failed to export report: " + e);
      }
    }
  };
  // Load Settings
  const loadSettings = async () => {
    try {
      const verifyVal = await invoke<string | null>("get_setting", { key: "auto_verify" });
      setAutoVerify(verifyVal === "true" || verifyVal === null);

      const hashAlgoVal = await invoke<string | null>("get_setting", { key: "hash_algorithm" });
      setHashAlgorithm(hashAlgoVal || "Blake3");

      const blockCloneVal = await invoke<string | null>("get_setting", { key: "enable_block_cloning" });
      setEnableBlockCloning(blockCloneVal === "true" || blockCloneVal === null);

      const limitVal = await invoke<string | null>("get_setting", { key: "speed_limit_kbps" });
      setSpeedLimitKbps(Number(limitVal) || 0);

      const explorerMenuVal = await invoke<string | null>("get_setting", { key: "explorer_context_menu" });
      setExplorerContextMenu(explorerMenuVal === "true");

      const soundsVal = await invoke<string | null>("get_setting", { key: "enable_sounds" });
      setEnableSounds(soundsVal !== "false");
    } catch (e) {
      console.error("Failed to load settings:", e);
    }
  };

  // Save Setting Helper
  const saveSetting = async (key: string, value: string) => {
    try {
      await invoke("set_setting", { key, value });
    } catch (e) {
      console.error("Failed to save setting:", e);
    }
  };

  const toggleExplorerContextMenu = async (enabled: boolean) => {
    try {
      if (enabled) {
        await invoke("register_explorer_context_menu");
      } else {
        await invoke("unregister_explorer_context_menu");
      }
      setExplorerContextMenu(enabled);
      saveSetting("explorer_context_menu", String(enabled));
    } catch (e) {
      console.error("Failed to toggle Explorer integration:", e);
      alert(`Error toggling Explorer integration: ${e}`);
    }
  };

  useEffect(() => {
    if (activeTab === "history") {
      refreshHistory(historyPage);
    }
  }, [historyPage, activeTab]);

  // Initialize event listeners
  useEffect(() => {
    refreshActiveJobs();
    refreshHistory(historyPage);
    loadSettings();

    // Listen for new transfer job
    const unlistenNewJob = listen<string>("transfer://new-job", () => {
      refreshActiveJobs();
    });

    // Listen for job status changes
    const unlistenStatusChanged = listen<[string, any]>("transfer://status-changed", (event) => {
      const [_, status] = event.payload;
      refreshActiveJobs();
      refreshHistory();
      
      if (status === "Done") {
        playChime("success");
      } else if (status === "Error" || (typeof status === "object" && status !== null && "Error" in status)) {
        playChime("error");
      }
    });

    // Listen for name conflicts
    const unlistenConflict = listen<ConflictInfo>("transfer://conflict", (event) => {
      setConflict(event.payload);
    });

    // Listen for new job configurations from Named Pipe (second instance)
    const unlistenConfigureNew = listen<[string[], boolean]>("transfer://configure-new", (event) => {
      const [paths, isMoveVal] = event.payload;
      if (paths && paths.length > 0) {
        setSrcPaths(paths);
        setIsMove(isMoveVal);
        setNewJobModal(true);
      }
    });

    // Fetch initial CLI arguments if application was opened with files/folders
    invoke<[string[], boolean]>("get_cli_args").then(([paths, isMoveVal]) => {
      if (paths && paths.length > 0) {
        setSrcPaths(paths);
        setIsMove(isMoveVal);
        setNewJobModal(true);
      }
    }).catch(e => console.error("Failed to load CLI args:", e));

    // Listen for file progress
    const unlistenFileProgress = listen<[string, string, number]>("transfer://file-progress", (event) => {
      const [jobId, fileSrc, bytesDone] = event.payload;
      
      // Update local active jobs directly to make progress extremely responsive
      setActiveJobs(prevJobs => 
        prevJobs.map(job => {
          if (job.id !== jobId) return job;
          const updatedFiles = job.files.map(f => {
            if (f.src === fileSrc) {
              return { ...f, bytes_done: bytesDone, status: "Copying" as const };
            }
            return f;
          });
          return { ...job, files: updatedFiles };
        })
      );

      // Update selected job details as well
      if (selectedJobRef.current && selectedJobRef.current.id === jobId) {
        setSelectedJob(prev => {
          if (!prev) return null;
          const updatedFiles = prev.files.map(f => {
            if (f.src === fileSrc) {
              return { ...f, bytes_done: bytesDone, status: "Copying" as const };
            }
            return f;
          });
          return { ...prev, files: updatedFiles };
        });
      }
    });

    // Listen for file status updates
    const unlistenFileStatus = listen<[string, string, string]>("transfer://file-status", (event) => {
      const [jobId, fileSrc, status] = event.payload;
      
      let parsedStatus: TransferFile["status"] = "Queued";
      if (status === "Copying" || status === "Verifying" || status === "Done" || status === "Skipped" || status === "Queued") {
        parsedStatus = status;
      } else {
        try {
          const parsed = JSON.parse(status);
          if (parsed && typeof parsed === "object" && "Error" in parsed) {
            parsedStatus = parsed;
          } else {
            parsedStatus = { Error: status };
          }
        } catch {
          parsedStatus = { Error: status };
        }
      }

      setActiveJobs(prevJobs => 
        prevJobs.map(job => {
          if (job.id !== jobId) return job;
          const updatedFiles = job.files.map(f => {
            if (f.src === fileSrc) {
              return { ...f, status: parsedStatus };
            }
            return f;
          });
          return { ...job, files: updatedFiles };
        })
      );

      if (selectedJobRef.current && selectedJobRef.current.id === jobId) {
        setSelectedJob(prev => {
          if (!prev) return null;
          const updatedFiles = prev.files.map(f => {
            if (f.src === fileSrc) {
              return { ...f, status: parsedStatus };
            }
            return f;
          });
          return { ...prev, files: updatedFiles };
        });
      }
    });

    // Listen for overall job progress updates
    const unlistenJobProgress = listen<[string, number, number]>("transfer://job-progress", (event) => {
      const [jobId, bytesDone, speedBps] = event.payload;

      setActiveJobs(prevJobs => 
        prevJobs.map(job => {
          if (job.id !== jobId) return job;
          return { ...job, bytes_done: bytesDone, speed_bps: speedBps };
        })
      );

      if (selectedJobRef.current && selectedJobRef.current.id === jobId) {
        setSelectedJob(prev => {
          if (!prev) return null;
          return { ...prev, bytes_done: bytesDone, speed_bps: speedBps };
        });
      }
    });

    // Listen for drag-drop events
    const unlistenDragDrop = listen<{ paths: string[] }>("tauri://drag-drop", (event) => {
      const paths = event.payload?.paths || [];
      if (paths.length > 0) {
        setSrcPaths(prev => {
          const unique = new Set([...prev, ...paths]);
          return Array.from(unique);
        });
        setNewJobModal(true);
        setActiveTab("dashboard");
      }
    });

    // Fallback polling for updates (in case socket drops or background named pipe jobs are added)
    const interval = setInterval(() => {
      refreshActiveJobs();
    }, 2500);

    return () => {
      clearInterval(interval);
      unlistenNewJob.then(f => f());
      unlistenStatusChanged.then(f => f());
      unlistenConflict.then(f => f());
      unlistenConfigureNew.then(f => f());
      unlistenFileProgress.then(f => f());
      unlistenFileStatus.then(f => f());
      unlistenJobProgress.then(f => f());
      unlistenDragDrop.then(f => f());
    };
  }, []);

  // Native Selectors
  const pickFiles = async () => {
    try {
      const files = await invoke<string[]>("select_files");
      setSrcPaths(prev => [...prev, ...files]);
    } catch (e) {
      console.warn("Cancelled picker:", e);
    }
  };

  const pickSourceDir = async () => {
    try {
      const dir = await invoke<string>("select_directory");
      if (dir) {
        setSrcPaths(prev => [...prev, dir]);
      }
    } catch (e) {
      console.warn("Cancelled picker:", e);
    }
  };

  const pickDir = async () => {
    try {
      const dir = await invoke<string>("select_directory");
      setDestDir(dir);
    } catch (e) {
      console.warn("Cancelled picker:", e);
    }
  };

  // Job Controls
  const handleAddJob = async () => {
    setNewJobError("");
    if (srcPaths.length === 0) {
      setNewJobError("Please select at least one source file or directory.");
      return;
    }
    if (!destDir) {
      setNewJobError("Please specify the destination directory.");
      return;
    }

    try {
      const jobId = await invoke<string>("add_transfer_job", {
        srcPaths,
        destDir,
        isMove,
      });
      setNewJobModal(false);
      setSrcPaths([]);
      setDestDir("");
      refreshActiveJobs();
      
      // Auto select the newly added job
      const details = await invoke<TransferJob | null>("get_job_details", { jobId });
      if (details) {
        setSelectedJob(details);
      }
    } catch (e: any) {
      setNewJobError(e.toString());
    }
  };

  const handlePauseJob = async (jobId: string) => {
    try {
      await invoke("pause_transfer_job", { jobId });
      refreshActiveJobs();
    } catch (e) {
      console.error(e);
    }
  };

  const handleResumeJob = async (jobId: string) => {
    try {
      await invoke("resume_transfer_job", { jobId });
      refreshActiveJobs();
    } catch (e) {
      console.error(e);
    }
  };

  const handleCancelJob = async (jobId: string) => {
    try {
      await invoke("cancel_transfer_job", { jobId });
      refreshActiveJobs();
      refreshHistory();
    } catch (e) {
      console.error(e);
    }
  };

  // Conflict Resolution
  const handleResolveConflict = async (resolution: string) => {
    if (!conflict) return;
    try {
      await invoke("resolve_conflict", {
        conflictId: conflict.conflict_id,
        resolution,
      });
      setConflict(null);
      refreshActiveJobs();
    } catch (e) {
      console.error("Conflict resolve error:", e);
    }
  };

  const getAdaptiveBufferValue = (size: number) => {
    if (size < 1024 * 1024) return 64 * 1024;
    if (size < 100 * 1024 * 1024) return 256 * 1024;
    if (size < 1024 * 1024 * 1024) return 1024 * 1024;
    return 4 * 1024 * 1024;
  };

  // Formatting utils
  const formatSize = (bytes: number) => {
    if (bytes === 0) return "0 Bytes";
    const k = 1024;
    const sizes = ["Bytes", "KB", "MB", "GB", "TB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
  };

  const formatSpeed = (bps: number) => {
    if (bps === 0) return "0 KB/s";
    const mb = bps / (1024 * 1024);
    if (mb >= 1) return mb.toFixed(2) + " MB/s";
    return (bps / 1024).toFixed(2) + " KB/s";
  };

  const formatDuration = (seconds: number) => {
    if (isNaN(seconds) || !isFinite(seconds) || seconds < 0) return "Estimating...";
    if (seconds < 60) return `${Math.round(seconds)}s`;
    const m = Math.floor(seconds / 60);
    const s = Math.round(seconds % 60);
    return `${m}m ${s}s`;
  };

  // ETA Calculation
  const getETA = (job: TransferJob) => {
    if (job.status === "Paused") return "Paused";
    if (job.status === "Done" || (typeof job.status === "object" && "Error" in job.status)) return "Finished";
    if (job.speed_bps === 0) return "Calculating...";
    const bytesLeft = job.bytes_total - job.bytes_done;
    const seconds = bytesLeft / job.speed_bps;
    return formatDuration(seconds);
  };

  // Progress Percent
  const getPercent = (job: TransferJob) => {
    if (job.bytes_total === 0) return 0;
    return Math.min(100, Math.round((job.bytes_done / job.bytes_total) * 100));
  };

  const getStatusString = (status: any) => {
    if (typeof status === "object" && status !== null && "Error" in status) {
      return `Error: ${status.Error}`;
    }
    return String(status);
  };

  return (
    <div className="app-container">
      {/* Sidebar Navigation */}
      <aside className="sidebar">
        <div>
          <div className="brand-section">
            <img className="brand-icon" src="/logo.png" alt="CopyTej" />
            <div className="brand-title">CopyTej</div>
          </div>
          <nav className="nav-list">
            <div
              className={`nav-item ${activeTab === "dashboard" ? "active" : ""}`}
              onClick={() => { setActiveTab("dashboard"); refreshActiveJobs(); }}
            >
              {/* Segoe Fluent Icon: Transfer */}
              <span className="nav-icon" aria-hidden="true">&#xE8AB;</span>
              <span>Transfers</span>
            </div>
            <div
              className={`nav-item ${activeTab === "topology" ? "active" : ""}`}
              onClick={() => { setActiveTab("topology"); refreshActiveJobs(); }}
            >
              {/* Segoe Fluent Icon: BranchFork */}
              <span className="nav-icon" aria-hidden="true">&#xE9BA;</span>
              <span>Pipeline Flow</span>
            </div>
            <div
              className={`nav-item ${activeTab === "history" ? "active" : ""}`}
              onClick={() => { setActiveTab("history"); refreshHistory(); }}
            >
              {/* Segoe Fluent Icon: History */}
              <span className="nav-icon" aria-hidden="true">&#xE81C;</span>
              <span>History</span>
            </div>
            <div
              className={`nav-item ${activeTab === "settings" ? "active" : ""}`}
              onClick={() => { setActiveTab("settings"); loadSettings(); }}
            >
              {/* Segoe Fluent Icon: Settings */}
              <span className="nav-icon" aria-hidden="true">&#xE713;</span>
              <span>Settings</span>
            </div>
          </nav>
        </div>
        <div className="version-info">CopyTej v0.3.0</div>
      </aside>

      {/* Main Panel */}
      <main className="main-content">
        <header className="top-bar">
          <h2 className="page-title">
            {activeTab === "dashboard" && "Active Transfers"}
            {activeTab === "topology" && "I/O Pipeline Flow"}
            {activeTab === "history" && "Transfer History"}
            {activeTab === "settings" && "Application Settings"}
          </h2>
          <div className="top-bar-actions">
            {(activeTab === "dashboard" || activeTab === "topology") && (
              <button className="btn btn-primary" onClick={() => setNewJobModal(true)}>
                {/* Segoe Fluent Icon: Add */}
                <span className="nav-icon" aria-hidden="true" style={{fontFamily: 'var(--fluent-icon-font)', fontSize: '14px'}}>&#xE710;</span> New Transfer
              </button>
            )}
          </div>
        </header>

        <div className="page-body">
          {/* Dashboard Tab */}
          {activeTab === "dashboard" && (
            <div className="dashboard-grid">
              {/* Left Column: Selected Job Details */}
              <div className="left-panel">
                {selectedJob ? (
                  <>
                    {/* Progress Summary Card */}
                    <section className="glass-card progress-summary">
                      <div className="progress-header-row">
                        <div className="job-meta-info">
                          <h3 className="card-title">
                            {selectedJob.is_move ? "Moving" : "Copying"} {selectedJob.files.length} items
                          </h3>
                          <span className="job-meta-path">To: {selectedJob.dest_dir}</span>
                        </div>
                        <span className="progress-percentage">{getPercent(selectedJob)}%</span>
                      </div>

                      <div className="progress-track">
                        <div
                          className="progress-bar-fill"
                          style={{ width: `${getPercent(selectedJob)}%` }}
                        ></div>
                      </div>

                      {/* Controls Row */}
                      <div style={{ display: "flex", gap: "10px", marginTop: "4px" }}>
                        {selectedJob.status === "Running" && (
                          <button className="btn btn-sm" onClick={() => handlePauseJob(selectedJob.id)}>
                            ⏸️ Pause
                          </button>
                        )}
                        {selectedJob.status === "Paused" && (
                          <button className="btn btn-sm btn-primary" onClick={() => handleResumeJob(selectedJob.id)}>
                            ▶️ Resume
                          </button>
                        )}
                        {(selectedJob.status === "Running" || selectedJob.status === "Paused") && (
                          <button className="btn btn-sm" style={{ borderColor: "var(--status-error)", color: "var(--status-error)" }} onClick={() => {
                            if (confirm("Are you sure you want to cancel this transfer? It cannot be resumed.")) {
                              handleCancelJob(selectedJob.id);
                            }
                          }}>
                            ⏹️ Cancel
                          </button>
                        )}
                      </div>

                      {/* Speed Indicator / Stats */}
                      <div className="progress-stats-row">
                        <div className="stat-item">
                          <span className="stat-label">Speed</span>
                          <span className="stat-value">{formatSpeed(selectedJob.speed_bps)}</span>
                        </div>
                        <div className="stat-item">
                          <span className="stat-label">Time Left</span>
                          <span className="stat-value">{getETA(selectedJob)}</span>
                        </div>
                        <div className="stat-item">
                          <span className="stat-label">Processed</span>
                          <span className="stat-value">
                            {formatSize(selectedJob.bytes_done)} / {formatSize(selectedJob.bytes_total)}
                          </span>
                        </div>
                        <div className="stat-item">
                          <span className="stat-label">Status</span>
                          <span className="stat-value" style={{ textTransform: "capitalize" }}>
                            {getStatusString(selectedJob.status)}
                          </span>
                        </div>
                      </div>
                    </section>

                    {/* Files List Card */}
                    <section className="glass-card file-list-card">
                      <div className="card-header">
                        <h3 className="card-title">Transfer Queue</h3>
                        <span style={{ fontSize: "0.85rem", color: "var(--text-muted)" }}>
                          {selectedJob.files.filter(f => f.status === "Done").length} / {selectedJob.files.length} Done
                        </span>
                      </div>
                      <div className="file-items-container">
                        {selectedJob.files.map((file, idx) => (
                          <div className="file-row" key={idx}>
                            <div className="file-info">
                              <span className="file-name" title={file.src}>{file.src.split(/[\\/]/).pop()}</span>
                              <span className="file-paths" title={`Source: ${file.src}\nDest: ${file.dest}`}>
                                {file.src} → {file.dest}
                              </span>
                            </div>
                            <div className="file-status-group">
                              <span className="file-size-progress">
                                {file.bytes_total === 0 ? "Folder" : `${formatSize(file.bytes_done)} / ${formatSize(file.bytes_total)}`}
                              </span>
                              <span className={`badge badge-${(typeof file.status === "object" ? "error" : file.status).toLowerCase()}`}>
                                {getStatusString(file.status)}
                              </span>
                            </div>
                          </div>
                        ))}
                      </div>
                    </section>
                  </>
                ) : (
                  <div className="glass-card empty-state" style={{ height: "100%" }}>
                    <div className="empty-icon">📁</div>
                    <h3 className="empty-title">No Active Transfers</h3>
                    <p>Start a new transfer or pipe a directory copying job.</p>
                  </div>
                )}
              </div>

              <ActiveQueueSidebar
                activeJobs={activeJobs}
                selectedJob={selectedJob}
                setSelectedJob={setSelectedJob}
                getPercent={getPercent}
                formatSpeed={formatSpeed}
                getStatusString={getStatusString}
              />
            </div>
          )}

          {/* Topology Tab */}
          {activeTab === "topology" && (
            <div className="dashboard-grid">
              {/* Left Column: Visual Pipeline Flow */}
              <div className="left-panel">
                {selectedJob ? (
                  <div className="glass-card topology-card" style={{ flexGrow: 1, display: "flex", flexDirection: "column", padding: "24px" }}>
                    <h3 className="card-title" style={{ marginBottom: "24px" }}>Real-Time Transfer Pipeline</h3>
                    <div className="topology-flow-grid">
                      {/* Left Node: Source */}
                      <div className="topology-node source-node">
                        <div className="node-header">
                          <span className="node-icon">📂</span>
                          <h4 className="node-title">Source Files</h4>
                        </div>
                        <div className="node-content">
                          <div className="topology-file-list">
                            {selectedJob.files.map((file, idx) => (
                              <div key={idx} className="topology-file-item">
                                <span className="file-icon">📄</span>
                                <span className="file-name" title={file.src}>{file.src.split(/[\\/]/).pop()}</span>
                                <span className="file-size">{formatSize(file.bytes_total)}</span>
                              </div>
                            ))}
                          </div>
                        </div>
                      </div>

                      {/* Connecting Line 1 */}
                      <div className="topology-flow-connector">
                        <svg className="flow-svg" viewBox="0 0 100 40">
                          <path
                            d="M 0,20 Q 50,0 100,20"
                            fill="none"
                            stroke="var(--accent-cyan)"
                            strokeWidth="2.5"
                            strokeDasharray="6 6"
                            className={selectedJob.status === "Running" ? "flow-dash-anim" : ""}
                          />
                          <text x="50%" y="10" textAnchor="middle" fill="var(--accent-cyan)" fontSize="6" fontWeight="bold">
                            READING ({formatSpeed(selectedJob.speed_bps)})
                          </text>
                        </svg>
                      </div>

                      {/* Center Node: Core Engine */}
                      <div className="topology-node core-node">
                        <div className="engine-core-orb">
                          <div className={`engine-orb-liquid ${selectedJob.status === "Running" ? "running" : ""}`}></div>
                          <div className="engine-orb-text">
                            <span className="engine-percent">{getPercent(selectedJob)}%</span>
                            <span className="engine-status">{getStatusString(selectedJob.status)}</span>
                          </div>
                        </div>
                        <div className="node-content" style={{ marginTop: "16px", width: "100%" }}>
                          <div className="engine-meta-row">
                            <span>Active File:</span>
                            <span className="engine-meta-val" style={{ color: "var(--accent-cyan)", fontWeight: "bold" }}>
                              {selectedJob.files.find(f => f.status === "Copying")?.src.split(/[\\/]/).pop() || "Idle"}
                            </span>
                          </div>
                          <span className="engine-meta-row">
                            <span>Hashing:</span>
                            <span className="engine-meta-val" style={{ color: "var(--accent-purple)", fontWeight: "bold" }}>{hashAlgorithm}</span>
                          </span>
                          <span className="engine-meta-row">
                            <span>Buffer Size:</span>
                            <span className="engine-meta-val">
                              {formatSize(getAdaptiveBufferValue(
                                selectedJob.files.find(f => f.status === "Copying")?.bytes_total || 0
                              ))}
                            </span>
                          </span>
                        </div>
                      </div>

                      {/* Connecting Line 2 */}
                      <div className="topology-flow-connector">
                        <svg className="flow-svg" viewBox="0 0 100 40">
                          <path
                            d="M 0,20 Q 50,40 100,20"
                            fill="none"
                            stroke="var(--accent-purple)"
                            strokeWidth="2.5"
                            strokeDasharray="6 6"
                            className={selectedJob.status === "Running" ? "flow-dash-anim" : ""}
                          />
                          <text x="50%" y="35" textAnchor="middle" fill="var(--accent-purple)" fontSize="6" fontWeight="bold">
                            WRITING
                          </text>
                        </svg>
                      </div>

                      {/* Right Node: Destination */}
                      <div className="topology-node dest-node">
                        <div className="node-header">
                          <span className="node-icon">📥</span>
                          <h4 className="node-title">Destination Disk</h4>
                        </div>
                        <div className="node-content">
                          <div className="dest-meta-path" title={selectedJob.dest_dir}>{selectedJob.dest_dir.split(/[\\/]/).pop()}</div>
                          <div className="dest-stats-container">
                            <div className="dest-stat-box">
                              <span className="dest-stat-num">
                                {selectedJob.files.filter(f => f.status === "Done").length}
                              </span>
                              <span className="dest-stat-lbl">Copied</span>
                            </div>
                            <div className="dest-stat-box">
                              <span className="dest-stat-num">
                                {selectedJob.files.filter(f => f.status === "Skipped").length}
                              </span>
                              <span className="dest-stat-lbl">Skipped</span>
                            </div>
                          </div>
                          <div className="dest-verified-list">
                            {selectedJob.files.filter(f => f.status === "Done").map((file, idx) => (
                              <div key={idx} className="verified-item">
                                <span className="check-icon">✓</span>
                                <span className="verified-name" title={file.src}>{file.src.split(/[\\/]/).pop()}</span>
                              </div>
                            ))}
                          </div>
                        </div>
                      </div>
                    </div>
                  </div>
                ) : (
                  <div className="glass-card empty-state" style={{ height: "100%" }}>
                    <div className="empty-icon">🧬</div>
                    <h3 className="empty-title">No Active Job Selected</h3>
                    <p>Select a running transfer from the queue sidebar to visualize the live I/O flow pipeline.</p>
                  </div>
                )}
              </div>

              <ActiveQueueSidebar
                activeJobs={activeJobs}
                selectedJob={selectedJob}
                setSelectedJob={setSelectedJob}
                getPercent={getPercent}
                formatSpeed={formatSpeed}
                getStatusString={getStatusString}
              />
            </div>
          )}

          {/* History Tab */}
          {activeTab === "history" && (
            <section className="glass-card">
              <div className="card-header" style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                <h3 className="card-title">Completed Transfers</h3>
                <div style={{ display: "flex", gap: "10px" }}>
                  <button className="btn btn-sm btn-secondary" onClick={() => refreshHistory(historyPage)}>🔄 Refresh</button>
                  <button className="btn btn-sm btn-danger" onClick={handleClearHistory}>🗑️ Clear All</button>
                </div>
              </div>
              <div className="history-table-container">
                {historyJobs.length === 0 ? (
                  <div className="empty-state">
                    <div className="empty-icon">🕒</div>
                    <h3 className="empty-title">No History Log</h3>
                    <p>All completed transfers will be detailed here.</p>
                  </div>
                ) : (
                  <>
                    <table className="history-table">
                      <thead>
                        <tr>
                          <th>Destination</th>
                          <th>Type</th>
                          <th>Status</th>
                          <th>Size</th>
                          <th>Speed</th>
                          <th>Date Started</th>
                          <th style={{ width: "50px", textAlign: "center" }}>Action</th>
                        </tr>
                      </thead>
                      <tbody>
                        {historyJobs.map(job => (
                          <tr key={job.id} style={{ cursor: "pointer" }} onClick={() => { setSelectedJob(job); setActiveTab("dashboard"); }}>
                            <td style={{ fontWeight: 500, color: "var(--text-title)" }}>
                              <span title={job.dest_dir}>{job.dest_dir}</span>
                            </td>
                            <td>
                              <span className="badge" style={{ background: "rgba(255,255,255,0.05)" }}>
                                {job.is_move ? "Move" : "Copy"}
                              </span>
                            </td>
                            <td>
                              <span
                                className={`badge badge-${(typeof job.status === "object" ? "error" : job.status).toLowerCase()}`}
                                title={typeof job.status === "object" && "Error" in job.status ? job.status.Error : ""}
                              >
                                {getStatusString(job.status)}
                              </span>
                            </td>
                            <td>{formatSize(job.bytes_total)}</td>
                            <td>{formatSpeed(job.speed_bps)}</td>
                            <td style={{ color: "var(--text-muted)", fontSize: "0.85rem" }}>
                              {job.started_at ? new Date(job.started_at * 1000).toLocaleString() : "Unknown"}
                            </td>
                            <td style={{ textAlign: "center", display: "flex", gap: "8px", justifyContent: "center" }}>
                              <button
                                className="btn btn-sm btn-icon"
                                style={{ background: "transparent", border: "none", color: "var(--accent-blue)", fontSize: "1.1rem", padding: "2px", cursor: "pointer" }}
                                onClick={(e) => handleExportJobReport(job.id, "csv", e)}
                                title="Export CSV Report"
                              >
                                📊
                              </button>
                              <button
                                className="btn btn-sm btn-icon"
                                style={{ background: "transparent", border: "none", color: "var(--accent-purple)", fontSize: "1.1rem", padding: "2px", cursor: "pointer" }}
                                onClick={(e) => handleExportJobReport(job.id, "json", e)}
                                title="Export JSON Report"
                              >
                                📄
                              </button>
                              <button
                                className="btn btn-sm btn-icon"
                                style={{ background: "transparent", border: "none", color: "var(--status-error)", fontSize: "1.1rem", padding: "2px", cursor: "pointer" }}
                                onClick={(e) => handleDeleteHistoryJob(job.id, e)}
                                title="Delete from history"
                              >
                                🗑️
                              </button>
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                    
                    {/* Pagination Controls */}
                    <div style={{ display: "flex", justifyContent: "center", alignItems: "center", gap: "15px", marginTop: "20px", padding: "10px 0" }}>
                      <button
                        className="btn btn-sm"
                        disabled={historyPage === 0}
                        onClick={() => setHistoryPage(p => Math.max(0, p - 1))}
                      >
                        ◀ Previous
                      </button>
                      <span style={{ fontSize: "0.9rem", color: "var(--text-muted)", fontWeight: "500" }}>
                        Page {historyPage + 1}
                      </span>
                      <button
                        className="btn btn-sm"
                        disabled={historyJobs.length < historyPerPage}
                        onClick={() => setHistoryPage(p => p + 1)}
                      >
                        Next ▶
                      </button>
                    </div>
                  </>
                )}
              </div>
            </section>
          )}

          {/* Settings Tab */}
          {activeTab === "settings" && (
            <section className="glass-card" style={{ maxWidth: "680px", margin: "0 auto" }}>
              <div className="settings-section">
                <h3 className="settings-section-title">Transfer Engine Config</h3>
                <div className="setting-row">
                  <div className="setting-info">
                    <span className="setting-title">Auto-Verify Copies</span>
                    <span className="setting-desc">Check files integrity by comparing checksums post-copy.</span>
                  </div>
                  <label className="checkbox-label">
                    <input
                      type="checkbox"
                      checked={autoVerify}
                      onChange={(e) => {
                        const val = e.target.checked;
                        setAutoVerify(val);
                        saveSetting("auto_verify", String(val));
                      }}
                    />
                  </label>
                </div>
                <div className="setting-row">
                  <div className="setting-info">
                    <span className="setting-title">Verification Algorithm</span>
                    <span className="setting-desc">High-performance hashing algorithm to use for checksum matching.</span>
                  </div>
                  <select
                    className="select-input"
                    value={hashAlgorithm}
                    onChange={(e) => {
                      const val = e.target.value;
                      setHashAlgorithm(val);
                      saveSetting("hash_algorithm", val);
                    }}
                  >
                    <option value="Blake3">Blake3 (Highly Secure, Multi-threaded)</option>
                    <option value="XxHash3">XXHash3 (Extremely Fast, Non-cryptographic)</option>
                    <option value="Md5">MD5 (Legacy Standard)</option>
                    <option value="Sha256">SHA-256 (Cryptographic Standard)</option>
                  </select>
                </div>
                <div className="setting-row">
                  <div className="setting-info">
                    <span className="setting-title">Enable NTFS/ReFS Block Cloning</span>
                    <span className="setting-desc">Enables instant, zero-byte file copying on supported filesystem volumes.</span>
                  </div>
                  <label className="checkbox-label">
                    <input
                      type="checkbox"
                      checked={enableBlockCloning}
                      onChange={(e) => {
                        const val = e.target.checked;
                        setEnableBlockCloning(val);
                        saveSetting("enable_block_cloning", String(val));
                      }}
                    />
                  </label>
                </div>
                <div className="setting-row">
                  <div className="setting-info">
                    <span className="setting-title">Global Speed Limit (KB/s)</span>
                    <span className="setting-desc">Throttles copy speed globally. Enter 0 for unlimited transfer speed.</span>
                  </div>
                  <input
                    className="form-input"
                    type="number"
                    style={{ maxWidth: "120px", textAlign: "right" }}
                    value={speedLimitKbps || ""}
                    placeholder="Unlimited"
                    onChange={(e) => {
                      const val = Math.max(0, Number(e.target.value));
                      setSpeedLimitKbps(val);
                      saveSetting("speed_limit_kbps", String(val));
                    }}
                  />
                </div>
              </div>

              <div className="settings-section">
                <h3 className="settings-section-title">System Integration</h3>
                <div className="setting-row">
                  <div className="setting-info">
                    <span className="setting-title">Windows Explorer Context Menu</span>
                    <span className="setting-desc">Integrates "Copy with CopyTej" into your right-click context menu (no admin rights required).</span>
                  </div>
                  <label className="checkbox-label">
                    <input
                      type="checkbox"
                      checked={explorerContextMenu}
                      onChange={(e) => toggleExplorerContextMenu(e.target.checked)}
                    />
                  </label>
                </div>
                <div className="setting-row">
                  <div className="setting-info">
                    <span className="setting-title">Enable Sound Effects</span>
                    <span className="setting-desc">Plays a success chime on completion and a warning sound on failures or skipped files.</span>
                  </div>
                  <label className="checkbox-label">
                    <input
                      type="checkbox"
                      checked={enableSounds}
                      onChange={(e) => {
                        const val = e.target.checked;
                        setEnableSounds(val);
                        saveSetting("enable_sounds", String(val));
                      }}
                    />
                  </label>
                </div>
              </div>
            </section>
          )}
        </div>
      </main>

      {/* New Job Modal Dialog */}
      {newJobModal && (
        <div className="modal-overlay">
          <div className="modal-content">
            <header className="modal-header">
              <h3 className="modal-title">Configure New Transfer</h3>
              <button className="btn btn-icon btn-sm" onClick={() => setNewJobModal(false)}>✖</button>
            </header>
            <div className="modal-body">
              {newJobError && (
                <div style={{ color: "var(--status-error)", padding: "12px", background: "rgba(244,63,94,0.1)", borderRadius: "8px", marginBottom: "16px", fontWeight: 500 }}>
                  ⚠️ {newJobError}
                </div>
              )}
              
              <div className="form-group">
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "8px" }}>
                  <label className="form-label" style={{ marginBottom: 0 }}>Source Paths</label>
                  {srcPaths.length > 0 && (
                    <button
                      className="btn btn-sm"
                      style={{ padding: "2px 8px", background: "rgba(244,63,94,0.15)", color: "var(--status-error)", border: "none", cursor: "pointer" }}
                      onClick={() => setSrcPaths([])}
                    >
                      Clear All
                    </button>
                  )}
                </div>
                <div className="input-row" style={{ gap: "8px" }}>
                  <input
                    className="form-input"
                    type="text"
                    placeholder="No files or folders selected"
                    value={srcPaths.join(", ")}
                    readOnly
                  />
                  <button className="btn" onClick={pickFiles} title="Select Files">📄 +Files</button>
                  <button className="btn" onClick={pickSourceDir} title="Select Folder">📁 +Folder</button>
                </div>
              </div>

              <div className="form-group">
                <label className="form-label">Destination Directory</label>
                <div className="input-row">
                  <input
                    className="form-input"
                    type="text"
                    placeholder="Select destination folder"
                    value={destDir}
                    onChange={(e) => setDestDir(e.target.value)}
                  />
                  <button className="btn" onClick={pickDir}>📂 Select</button>
                </div>
              </div>

              <div className="form-group">
                <label className="form-label">Transfer Mode</label>
                <div className="radio-group">
                  <label className="radio-label">
                    <input
                      type="radio"
                      name="jobType"
                      checked={!isMove}
                      onChange={() => setIsMove(false)}
                    />
                    Copy Files
                  </label>
                  <label className="radio-label">
                    <input
                      type="radio"
                      name="jobType"
                      checked={isMove}
                      onChange={() => setIsMove(true)}
                    />
                    Move Files
                  </label>
                </div>
              </div>
            </div>
            <footer className="modal-footer">
              <button className="btn" onClick={() => setNewJobModal(false)}>Cancel</button>
              <button className="btn btn-primary" onClick={handleAddJob}>Start Transfer</button>
            </footer>
          </div>
        </div>
      )}

      {/* Conflict Resolution Dialogue Popup overlay */}
      {conflict && (
        <div className="modal-overlay">
          <div className="modal-content conflict-modal-content">
            <header className="modal-header" style={{ borderBottomColor: "rgba(244,63,94,0.15)" }}>
              <h3 className="modal-title" style={{ color: "var(--status-error)" }}>⚠️ File Conflict Detected</h3>
            </header>
            <div className="modal-body">
              <p style={{ marginBottom: "20px", fontSize: "0.95rem" }}>
                A filename collision was encountered. Please select a resolution strategy:
              </p>
              
              <div className="conflict-comparison">
                {/* Source File details */}
                <div className="conflict-file-card src">
                  <span className="conflict-card-label">Source File (Copying)</span>
                  <span className="conflict-path" title={conflict.file_path}>{conflict.file_path.split(/[\\/]/).pop()}</span>
                  <div className="conflict-detail-row">
                    <span>Size:</span>
                    <span className="conflict-detail-val">{formatSize(conflict.src_size)}</span>
                  </div>
                  <div className="conflict-detail-row">
                    <span>Modified:</span>
                    <span className="conflict-detail-val">
                      {new Date(conflict.src_modified * 1000).toLocaleTimeString()}
                    </span>
                  </div>
                </div>

                {/* Destination File details */}
                <div className="conflict-file-card dest">
                  <span className="conflict-card-label">Destination File (Existing)</span>
                  <span className="conflict-path" title={conflict.file_path}>{conflict.file_path.split(/[\\/]/).pop()}</span>
                  <div className="conflict-detail-row">
                    <span>Size:</span>
                    <span className="conflict-detail-val">{formatSize(conflict.dest_size)}</span>
                  </div>
                  <div className="conflict-detail-row">
                    <span>Modified:</span>
                    <span className="conflict-detail-val">
                      {new Date(conflict.dest_modified * 1000).toLocaleTimeString()}
                    </span>
                  </div>
                </div>
              </div>

              {/* Actions Grid */}
              <div className="conflict-actions-grid" style={{ gridTemplateColumns: "repeat(3, 1fr)" }}>
                <button className="btn conflict-action-btn" style={{ borderColor: "var(--status-error)" }} onClick={() => handleResolveConflict("Overwrite")}>
                  <span className="conflict-action-title" style={{ color: "var(--status-error)" }}>Overwrite</span>
                  <span className="conflict-action-subtitle">Replace target file</span>
                </button>
                <button className="btn conflict-action-btn" onClick={() => handleResolveConflict("Skip")}>
                  <span className="conflict-action-title">Skip</span>
                  <span className="conflict-action-subtitle">Ignore this item</span>
                </button>
                <button className="btn conflict-action-btn" style={{ borderColor: "var(--accent-purple)" }} onClick={() => handleResolveConflict("Rename")}>
                  <span className="conflict-action-title" style={{ color: "var(--accent-purple)" }}>Rename</span>
                  <span className="conflict-action-subtitle">Keep both files</span>
                </button>

                <button className="btn conflict-action-btn" style={{ borderColor: "var(--status-warning)" }} onClick={() => handleResolveConflict("OverwriteOlder")}>
                  <span className="conflict-action-title" style={{ color: "var(--status-warning)" }}>Overwrite Older</span>
                  <span className="conflict-action-subtitle">Only if source is newer</span>
                </button>
                <button className="btn conflict-action-btn" style={{ borderColor: "var(--accent-cyan)" }} onClick={() => handleResolveConflict("SkipSameSizeDate")}>
                  <span className="conflict-action-title" style={{ color: "var(--accent-cyan)" }}>Skip if Same</span>
                  <span className="conflict-action-subtitle">Skip if size/date match</span>
                </button>
                <div style={{ content: '""' }}></div>

                <button className="btn conflict-action-btn" style={{ borderColor: "var(--status-error)", opacity: 0.85 }} onClick={() => {
                  if (confirm("Are you sure you want to overwrite ALL conflicting files? This cannot be undone.")) {
                    handleResolveConflict("OverwriteAll");
                  }
                }}>
                  <span className="conflict-action-title" style={{ color: "var(--status-error)" }}>Overwrite All</span>
                  <span className="conflict-action-subtitle">Always overwrite</span>
                </button>
                <button className="btn conflict-action-btn" style={{ opacity: 0.85 }} onClick={() => handleResolveConflict("SkipAll")}>
                  <span className="conflict-action-title">Skip All</span>
                  <span className="conflict-action-subtitle">Always skip</span>
                </button>
                <button className="btn conflict-action-btn" style={{ borderColor: "var(--accent-purple)", opacity: 0.85 }} onClick={() => handleResolveConflict("RenameAll")}>
                  <span className="conflict-action-title" style={{ color: "var(--accent-purple)" }}>Rename All</span>
                  <span className="conflict-action-subtitle">Always auto-rename</span>
                </button>

                <button className="btn conflict-action-btn" style={{ borderColor: "var(--status-warning)", opacity: 0.85 }} onClick={() => handleResolveConflict("OverwriteOlderAll")}>
                  <span className="conflict-action-title" style={{ color: "var(--status-warning)" }}>Overwrite Older (All)</span>
                  <span className="conflict-action-subtitle">Always check modified</span>
                </button>
                <button className="btn conflict-action-btn" style={{ borderColor: "var(--accent-cyan)", opacity: 0.85 }} onClick={() => handleResolveConflict("SkipSameSizeDateAll")}>
                  <span className="conflict-action-title" style={{ color: "var(--accent-cyan)" }}>Skip if Same (All)</span>
                  <span className="conflict-action-subtitle">Always check size/date</span>
                </button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default App;
