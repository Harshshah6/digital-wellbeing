import { useState, useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { version } from "../package.json";
import {
  Activity,
  Clock,
  Sun,
  Moon
} from "lucide-react";
import {
  AreaChart,
  Area,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
  BarChart,
  Bar
} from "recharts";

import "./index.css";

// Utility: Format seconds into readable duration
function formatDuration(totalSeconds: number): string {
  if (totalSeconds <= 0) return "0m";
  const h = Math.floor(totalSeconds / 3600);
  const m = Math.floor((totalSeconds % 3600) / 60);

  if (h > 0) {
    return `${h}h ${m}m`;
  }
  return `${m}m`;
}

// Apple Tooltip design
const AppleAreaTooltip = ({ active, payload, showInHrs }: any) => {
  if (active && payload && payload.length) {
    const value = payload[0].value;
    let displayValue = `${value}m`;
    if (showInHrs) {
      const hrs = value / 60;
      displayValue = hrs % 1 === 0 ? `${hrs}h` : `${hrs.toFixed(1)}h`;
    }
    return (
      <div
        style={{
          padding: '8px 12px',
          borderRadius: '8px',
          fontSize: '12px',
          border: '1px solid var(--colors-hairline)',
          background: 'var(--colors-canvas)',
          boxShadow: '0 4px 12px rgba(0,0,0,0.06)'
        }}
      >
        <p style={{ color: 'var(--colors-ink-muted-48)', marginBottom: '2px' }}>{payload[0].payload.hour || payload[0].payload.label}</p>
        <p style={{ fontWeight: 600, color: 'var(--colors-primary)' }}>{displayValue} active</p>
      </div>
    );
  }
  return null;
};

export default function App() {
  const [activeTab, setActiveTab] = useState<"dashboard" | "analytics" | "settings">("dashboard");
  const [theme, setTheme] = useState<"dark" | "light">(() => {
    const saved = localStorage.getItem("dw-theme");
    return (saved === "dark" || saved === "light") ? saved : "light";
  });

  // selectedDate is the active date context. todayDate holds the real system today's date.
  const [selectedDate, setSelectedDate] = useState(() => new Date().toISOString().split("T")[0]);
  const [todayDate, setTodayDate] = useState(() => new Date().toISOString().split("T")[0]);

  // Switchable graph state: "day" (hourly Area), "week" (daily Bar last 7 days), "month" (daily Bar last 30 days), "year" (monthly Bar last 12 months)
  const [graphView, setGraphView] = useState<"day" | "week" | "month" | "year">("day");

  // Telemetry state
  const [topApps, setTopApps] = useState<any[]>([]);
  const [categories, setCategories] = useState<any[]>([]);
  const [timeline, setTimeline] = useState<any[]>([]);
  const [heatmapData, setHeatmapData] = useState<any[]>([]);
  const [yearlyData, setYearlyData] = useState<any[]>([]);
  const [avgStats, setAvgStats] = useState<any>(null);

  const [searchTerm, setSearchTerm] = useState("");

  // Autostart state
  const [autostartEnabled, setAutostartEnabled] = useState(true);

  // Idle monitor state
  const [idleMonitoring, setIdleMonitoring] = useState(false);

  // Sync date every minute, auto-advance selectedDate if it matches todayDate
  useEffect(() => {
    const timer = setInterval(() => {
      const newToday = new Date().toISOString().split("T")[0];
      setTodayDate(newToday);
      setSelectedDate(current => {
        if (current === todayDate) {
          return newToday;
        }
        return current;
      });
    }, 60000);
    return () => clearInterval(timer);
  }, [todayDate]);

  // Fetch telemetry based on selectedDate
  useEffect(() => {
    const fetchTelemetry = async () => {
      try {
        const appsRes = await invoke<any[]>("get_top_apps", { dateStr: selectedDate, limit: 10 });
        setTopApps(appsRes || []);

        const catRes = await invoke<any[]>("get_category_distribution", { dateStr: selectedDate });
        setCategories(catRes || []);

        const timeRes = await invoke<any[]>("get_hourly_timeline", { dateStr: selectedDate });
        const formattedTimeline = (timeRes || []).map(seg => {
          const h = seg.hour;
          const ampm = h >= 12 ? 'PM' : 'AM';
          const displayHour = h % 12 === 0 ? 12 : h % 12;
          return {
            hour: `${displayHour} ${ampm}`,
            Minutes: Math.round(seg.total_seconds / 60)
          };
        });
        setTimeline(formattedTimeline);

        const heatRes = await invoke<any[]>("get_heatmap_data");
        setHeatmapData(heatRes || []);

        // Yearly data
        const yearRes = await invoke<any[]>("get_yearly_data", { dateStr: selectedDate });
        setYearlyData(yearRes || []);

        // Avg screen time stats
        const avgRes = await invoke<any>("get_avg_screen_time");
        setAvgStats(avgRes || null);
      } catch (err) {
        console.error("Error loading telemetry:", err);
      }
    };

    fetchTelemetry();

    // Live telemetry sync interval
    const interval = setInterval(fetchTelemetry, 5000);
    return () => clearInterval(interval);
  }, [selectedDate]);

  // Handle Theme
  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
    localStorage.setItem("dw-theme", theme);
  }, [theme]);

  // Check autostart status on mount
  useEffect(() => {
    invoke<boolean>("get_autostart_enabled")
      .then(setAutostartEnabled)
      .catch(() => { });
    invoke<boolean>("get_idle_monitoring")
      .then(setIdleMonitoring)
      .catch(() => { });
  }, []);

  // Total screen time calculation
  const totalScreenTimeSecs = useMemo(() => {
    return topApps.reduce((acc, app) => acc + app.total_seconds, 0);
  }, [topApps]);

  // Toggle autostart
  const toggleAutostart = async (enabled: boolean) => {
    try {
      await invoke("set_autostart_enabled", { enabled });
      setAutostartEnabled(enabled);
    } catch (e) {
      console.error("Failed to toggle autostart:", e);
    }
  };

  // Toggle idle monitoring
  const toggleIdleMonitoring = async (enabled: boolean) => {
    try {
      await invoke("set_idle_monitoring", { enabled });
      setIdleMonitoring(enabled);
    } catch (e) {
      console.error("Failed to toggle idle monitoring:", e);
    }
  };


  // Export Data using native Rust save dialog
  const exportTelemetry = async (format: "json" | "csv") => {
    const dataToExport = {
      summaryDate: selectedDate,
      totalScreenTimeSeconds: totalScreenTimeSecs,
      apps: topApps,
      categories
    };

    let content = "";
    let filename = "";
    if (format === "json") {
      content = JSON.stringify(dataToExport, null, 2);
      filename = `dw_report_${selectedDate}.json`;
    } else {
      content = "Application,Executable,Category,Usage (Seconds)\n";
      topApps.forEach(app => {
        content += `"${app.display_name}","${app.executable_name}","${app.category}",${app.total_seconds}\n`;
      });
      filename = `dw_report_${selectedDate}.csv`;
    }

    try {
      const savedPath = await invoke<string>("export_data", { content, filename });
      alert(`Report exported successfully to:\n${savedPath}`);
    } catch (err: any) {
      if (err !== "Save cancelled") {
        console.error("Export failed:", err);
        alert(`Export failed: ${err}`);
      }
    }
  };

  // Heatmap mapping (120 days)
  const heatmapGridData = useMemo(() => {
    const grid = [];
    const now = new Date();
    for (let i = 119; i >= 0; i--) {
      const d = new Date();
      d.setDate(now.getDate() - i);
      const dateStr = d.toISOString().split("T")[0];
      const match = heatmapData.find(p => p.date === dateStr);
      const seconds = match ? match.count : 0;

      let level = 0;
      if (seconds > 0 && seconds <= 1800) level = 1;
      else if (seconds > 1800 && seconds <= 7200) level = 2;
      else if (seconds > 7200 && seconds <= 14400) level = 3;
      else if (seconds > 14400) level = 4;

      grid.push({ dateStr, level, seconds });
    }
    return grid;
  }, [heatmapData]);

  // History Graph Data (last 7 or 30 days)
  const historyGraphData = useMemo(() => {
    const data = [];
    const baseDate = new Date(selectedDate);
    const count = graphView === "week" ? 7 : 30;
    for (let i = count - 1; i >= 0; i--) {
      const d = new Date(baseDate);
      d.setDate(baseDate.getDate() - i);
      const dateStr = d.toISOString().split("T")[0];
      const match = heatmapData.find(p => p.date === dateStr);
      const mins = match ? Math.round(match.count / 60) : 0;

      let label = "";
      if (graphView === "week") {
        label = d.toLocaleDateString(undefined, { weekday: 'short' });
      } else {
        label = d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
      }
      data.push({
        dateStr,
        label,
        Minutes: mins
      });
    }
    return data;
  }, [selectedDate, heatmapData, graphView]);

  // Yearly graph data: map yearlyData to chart-friendly format (hours)
  const yearlyGraphData = useMemo(() => {
    return yearlyData.map(d => ({
      label: d.month_label,
      month: d.month,
      Hours: parseFloat((d.total_minutes / 60).toFixed(1)),
    }));
  }, [yearlyData]);

  // Helper to format selected date nicely
  const formattedSelectedDate = useMemo(() => {
    const d = new Date(selectedDate);
    return d.toLocaleDateString(undefined, { weekday: 'long', year: 'numeric', month: 'long', day: 'numeric' });
  }, [selectedDate]);

  return (
    <div className="app-container">
      {/* Sub-nav-frosted — now the top-level navigation bar */}
      <header className="sub-nav-frosted">
        <div style={{ display: 'flex', alignItems: 'center', gap: '12px' }}>
          <Activity size={16} color="var(--colors-primary)" />
          <span className="sub-nav-title">DigitalWellbeing</span>
        </div>
        <div className="sub-nav-actions">
          <span className={`sub-nav-tab ${activeTab === 'dashboard' ? 'active' : ''}`} onClick={() => setActiveTab('dashboard')}>Overview</span>
          <span className={`sub-nav-tab ${activeTab === 'analytics' ? 'active' : ''}`} onClick={() => setActiveTab('analytics')}>Insights</span>
          <span className={`sub-nav-tab ${activeTab === 'settings' ? 'active' : ''}`} onClick={() => setActiveTab('settings')}>Settings</span>

          {/* Theme toggle */}
          <div
            className="theme-toggle-wrapper"
            onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}
            style={{ cursor: 'pointer' }}
          >
            {theme === 'dark' ? <Moon size={14} color="var(--colors-ink-muted-48)" /> : <Sun size={14} color="var(--colors-ink-muted-48)" />}
            <div className={`theme-toggle-track ${theme === 'dark' ? 'active' : ''}`}>
              <div className="theme-toggle-thumb" />
            </div>
          </div>
        </div>
      </header>

      {/* Main Scrollable Viewport */}
      <div className="main-viewport">

        {activeTab === "dashboard" && (
          <>
            {/* Tile 1: Core Cards */}
            <section className="viewport-tile light">
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', flexWrap: 'wrap', gap: '16px', width: '100%' }}>
                <div>
                  <h1 className="hero-display">Performance Overview</h1>
                  <p className="lead-subcopy" style={{ marginTop: '8px' }}>
                    {formattedSelectedDate} {selectedDate === todayDate ? "(Today)" : ""}
                  </p>
                </div>
                {selectedDate !== todayDate && (
                  <button
                    className="button-secondary-pill"
                    onClick={() => setSelectedDate(todayDate)}
                    style={{ fontSize: '12px', padding: '6px 12px' }}
                  >
                    Go to Today
                  </button>
                )}
              </div>

              {/* Grid of Store Cards */}
              <div className="store-grid-container" style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(240px, 1fr))' }}>
                <div className="store-utility-card">
                  <div className="card-header">
                    <div>
                      <span className="card-subtitle">SCREEN TIME</span>
                      <h3 className="card-title" style={{ fontSize: '24px', marginTop: '6px' }}>{formatDuration(totalScreenTimeSecs)}</h3>
                    </div>
                    <Clock size={18} color="var(--colors-primary)" />
                  </div>
                  <span style={{ fontSize: '13px', color: 'var(--colors-ink-muted-48)' }}>Today's Active Duration</span>
                </div>

                <div className="store-utility-card">
                  <div className="card-header">
                    <div>
                      <span className="card-subtitle">APPLICATIONS</span>
                      <h3 className="card-title" style={{ fontSize: '24px', marginTop: '6px' }}>{topApps.length}</h3>
                    </div>
                    <Activity size={18} color="var(--colors-primary)" />
                  </div>
                  <span style={{ fontSize: '13px', color: 'var(--colors-ink-muted-48)' }}>Unique Apps Tracked</span>
                </div>
              </div>
            </section>

            {/* Tile 2: Charts */}
            <section className="viewport-tile parchment">
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: '16px', width: '100%' }}>
                <div>
                  <h2 className="hero-display" style={{ fontSize: '28px' }}>Interactions</h2>
                  <p className="lead-subcopy" style={{ fontSize: '15px', marginTop: '4px' }}>Screen time charts and categorical activity distributions.</p>
                </div>

                {/* Switchable Graph segmented buttons */}
                <div className="segmented-pill-container" style={{ background: 'var(--colors-canvas-parchment)' }}>
                  <button
                    className={`segmented-pill-button ${graphView === 'day' ? 'active' : ''}`}
                    onClick={() => setGraphView('day')}
                  >
                    Daily
                  </button>
                  <button
                    className={`segmented-pill-button ${graphView === 'week' ? 'active' : ''}`}
                    onClick={() => setGraphView('week')}
                  >
                    Weekly
                  </button>
                  <button
                    className={`segmented-pill-button ${graphView === 'month' ? 'active' : ''}`}
                    onClick={() => setGraphView('month')}
                  >
                    Monthly
                  </button>
                  <button
                    className={`segmented-pill-button ${graphView === 'year' ? 'active' : ''}`}
                    onClick={() => setGraphView('year')}
                  >
                    Yearly
                  </button>
                </div>
              </div>

              <div className="store-grid-container" style={{ gridTemplateColumns: '1fr', marginTop: '16px' }}>
                {/* Switchable history chart */}
                <div className="store-utility-card">
                  <span className="card-subtitle" style={{ marginBottom: '16px', display: 'block', textTransform: 'uppercase' }}>
                    {graphView === 'day' ? 'HOURLY ACTIVITY (MINUTES)'
                      : graphView === 'week' ? 'DAILY ACTIVITY — LAST 7 DAYS'
                      : graphView === 'month' ? 'DAILY ACTIVITY — LAST 30 DAYS'
                      : 'MONTHLY ACTIVITY — LAST 12 MONTHS (HOURS)'}
                  </span>
                  <div style={{ width: '100%', height: '240px' }}>
                    {graphView === 'day' ? (
                      timeline.length > 0 ? (
                        <ResponsiveContainer width="100%" height="100%">
                          <AreaChart data={timeline} margin={{ top: 10, right: 10, left: -25, bottom: 0 }}>
                            <defs>
                              <linearGradient id="colorMinutes" x1="0" y1="0" x2="0" y2="1">
                                <stop offset="5%" stopColor="var(--colors-primary)" stopOpacity={0.15} />
                                <stop offset="95%" stopColor="var(--colors-primary)" stopOpacity={0} />
                              </linearGradient>
                            </defs>
                            <XAxis dataKey="hour" stroke="var(--colors-ink-muted-48)" fontSize={10} tickLine={false} axisLine={false} />
                            <YAxis stroke="var(--colors-ink-muted-48)" fontSize={10} tickLine={false} axisLine={false} />
                            <Tooltip content={<AppleAreaTooltip />} />
                            <Area type="monotone" dataKey="Minutes" stroke="var(--colors-primary)" strokeWidth={2} fillOpacity={1} fill="url(#colorMinutes)" />
                          </AreaChart>
                        </ResponsiveContainer>
                      ) : (
                        <div style={{ display: 'flex', height: '100%', justifyContent: 'center', alignItems: 'center', color: 'var(--colors-ink-muted-48)', fontSize: '13px' }}>
                          No tracking metrics captured for this day.
                        </div>
                      )
                    ) : graphView === 'year' ? (
                      <ResponsiveContainer width="100%" height="100%">
                        <BarChart data={yearlyGraphData} margin={{ top: 10, right: 10, left: -25, bottom: 0 }}>
                          <XAxis dataKey="label" stroke="var(--colors-ink-muted-48)" fontSize={10} tickLine={false} axisLine={false} />
                          <YAxis stroke="var(--colors-ink-muted-48)" fontSize={10} tickLine={false} axisLine={false} />
                          <Tooltip
                            content={({ active, payload }: any) => {
                              if (active && payload && payload.length) {
                                const v = payload[0].value;
                                return (
                                  <div style={{ padding: '8px 12px', borderRadius: '8px', fontSize: '12px', border: '1px solid var(--colors-hairline)', background: 'var(--colors-canvas)', boxShadow: '0 4px 12px rgba(0,0,0,0.06)' }}>
                                    <p style={{ color: 'var(--colors-ink-muted-48)', marginBottom: '2px' }}>{payload[0].payload.label}</p>
                                    <p style={{ fontWeight: 600, color: 'var(--colors-primary)' }}>{v}h active</p>
                                  </div>
                                );
                              }
                              return null;
                            }}
                          />
                          <Bar
                            dataKey="Hours"
                            fill="var(--colors-primary)"
                            radius={[4, 4, 0, 0]}
                            opacity={0.85}
                          />
                        </BarChart>
                      </ResponsiveContainer>
                    ) : (
                      <ResponsiveContainer width="100%" height="100%">
                        <BarChart data={historyGraphData} margin={{ top: 10, right: 10, left: -25, bottom: 0 }}>
                          <XAxis dataKey="label" stroke="var(--colors-ink-muted-48)" fontSize={10} tickLine={false} axisLine={false} />
                          <YAxis stroke="var(--colors-ink-muted-48)" fontSize={10} tickLine={false} axisLine={false} />
                          <Tooltip content={<AppleAreaTooltip showInHrs={true} />} />
                          <Bar
                            dataKey="Minutes"
                            fill="var(--colors-primary)"
                            radius={[4, 4, 0, 0]}
                            onClick={(data: any) => {
                              if (data && data.dateStr) {
                                setSelectedDate(data.dateStr);
                                setGraphView("day");
                              }
                            }}
                            style={{ cursor: 'pointer' }}
                          />
                        </BarChart>
                      </ResponsiveContainer>
                    )}
                  </div>
                </div>
              </div>
            </section>

            {/* Tile 3: App rankings */}
            <section className="viewport-tile light">
              <div>
                <h2 className="hero-display" style={{ fontSize: '28px' }}>Application Rankings</h2>
                <p className="lead-subcopy" style={{ fontSize: '15px', marginTop: '4px' }}>Ranking of most actively used processes on this device.</p>
              </div>

              <div className="store-utility-card">
                <div style={{ display: 'flex', flexDirection: 'column', gap: '20px' }}>
                  {topApps.length > 0 ? (
                    topApps.map((app, index) => {
                      const percentage = totalScreenTimeSecs > 0 ? Math.round((app.total_seconds / totalScreenTimeSecs) * 100) : 0;
                      return (
                        <div key={app.executable_name} style={{ display: 'flex', alignItems: 'center', gap: '20px' }}>
                          <div style={{
                            width: '24px',
                            height: '24px',
                            borderRadius: '50%',
                            background: 'var(--colors-canvas-parchment)',
                            display: 'flex',
                            alignItems: 'center',
                            justifyContent: 'center',
                            fontSize: '11px',
                            fontWeight: 700,
                            color: 'var(--colors-ink-muted-80)'
                          }}>
                            {index + 1}
                          </div>
                          <div style={{ flex: 1 }}>
                            <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '6px' }}>
                              <span style={{ fontSize: '14px', fontWeight: 600, color: 'var(--colors-ink)' }}>{app.display_name}</span>
                              <span style={{ fontSize: '13px', color: 'var(--colors-ink-muted-80)', fontWeight: 500 }}>{formatDuration(app.total_seconds)} ({percentage}%)</span>
                            </div>
                            <div className="capsule-progress-bar">
                              <div className="capsule-progress-fill" style={{ width: `${percentage}%` }}></div>
                            </div>
                          </div>
                        </div>
                      );
                    })
                  ) : (
                    <div style={{ textAlign: 'center', padding: '20px 0', color: 'var(--colors-ink-muted-48)', fontSize: '13px' }}>
                      No tracking metrics captured for this day.
                    </div>
                  )}
                </div>
              </div>
            </section>
          </>
        )}

        {activeTab === "analytics" && (
          <>
            {/* Tile 1: Heatmap */}
            <section className="viewport-tile light">
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', flexWrap: 'wrap', gap: '16px', width: '100%' }}>
                <div>
                  <h1 className="hero-display">Consistency Insights</h1>
                  <p className="lead-subcopy" style={{ marginTop: '8px' }}>
                    Click a date below to view detailed breakdown for {formattedSelectedDate} {selectedDate === todayDate ? "(Today)" : ""}
                  </p>
                </div>
                {selectedDate !== todayDate && (
                  <button
                    className="button-secondary-pill"
                    onClick={() => setSelectedDate(todayDate)}
                    style={{ fontSize: '12px', padding: '6px 12px' }}
                  >
                    Reset to Today
                  </button>
                )}
              </div>

              <div className="heatmap-calendar-card">
                <span className="card-subtitle" style={{ marginBottom: '16px', display: 'block' }}>DAILY SCREEN TIME HEATMAP</span>
                <div className="heatmap-grid" style={{ padding: '8px 0' }}>
                  {heatmapGridData.map((day, index) => (
                    <div
                      key={index}
                      className={`heatmap-cell level-${day.level}`}
                      title={`${day.dateStr}: ${formatDuration(day.seconds)} tracked`}
                      onClick={() => setSelectedDate(day.dateStr)}
                      style={{
                        cursor: 'pointer',
                        border: day.dateStr === selectedDate ? '1.5px solid var(--colors-primary)' : undefined,
                        boxSizing: 'border-box'
                      }}
                    />
                  ))}
                </div>
                <div style={{ display: 'flex', justifyContent: 'flex-end', gap: '8px', fontSize: '11px', color: 'var(--colors-ink-muted-48)', marginTop: '12px', alignItems: 'center' }}>
                  <span>Less</span>
                  <div className="heatmap-cell level-0" style={{ width: '10px', height: '10px' }}></div>
                  <div className="heatmap-cell level-1" style={{ width: '10px', height: '10px' }}></div>
                  <div className="heatmap-cell level-2" style={{ width: '10px', height: '10px' }}></div>
                  <div className="heatmap-cell level-3" style={{ width: '10px', height: '10px' }}></div>
                  <div className="heatmap-cell level-4" style={{ width: '10px', height: '10px' }}></div>
                  <span>More</span>
                </div>
              </div>
            </section>

            {/* Tile 2: Avg Screen Time Stats */}
            <section className="viewport-tile parchment">
              <div>
                <h2 className="hero-display" style={{ fontSize: '28px' }}>Screen Time Averages</h2>
                <p className="lead-subcopy" style={{ fontSize: '15px', marginTop: '4px' }}>Average screen time computed across all {avgStats?.tracked_days ?? 0} tracked days.</p>
              </div>

              <div className="store-grid-container" style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(200px, 1fr))' }}>
                <div className="store-utility-card">
                  <div className="card-header">
                    <div>
                      <span className="card-subtitle">DAILY AVG</span>
                      <h3 className="card-title" style={{ fontSize: '24px', marginTop: '6px' }}>
                        {avgStats ? formatDuration(avgStats.daily_avg_seconds) : '—'}
                      </h3>
                    </div>
                    <Clock size={18} color="var(--colors-primary)" />
                  </div>
                  <span style={{ fontSize: '13px', color: 'var(--colors-ink-muted-48)' }}>Average per tracked day</span>
                </div>

                <div className="store-utility-card">
                  <div className="card-header">
                    <div>
                      <span className="card-subtitle">WEEKLY AVG</span>
                      <h3 className="card-title" style={{ fontSize: '24px', marginTop: '6px' }}>
                        {avgStats ? formatDuration(avgStats.weekly_avg_seconds) : '—'}
                      </h3>
                    </div>
                    <Activity size={18} color="var(--colors-primary)" />
                  </div>
                  <span style={{ fontSize: '13px', color: 'var(--colors-ink-muted-48)' }}>Average per week</span>
                </div>

                <div className="store-utility-card">
                  <div className="card-header">
                    <div>
                      <span className="card-subtitle">MONTHLY AVG</span>
                      <h3 className="card-title" style={{ fontSize: '24px', marginTop: '6px' }}>
                        {avgStats ? formatDuration(avgStats.monthly_avg_seconds) : '—'}
                      </h3>
                    </div>
                    <Clock size={18} color="var(--colors-primary)" />
                  </div>
                  <span style={{ fontSize: '13px', color: 'var(--colors-ink-muted-48)' }}>Average per month</span>
                </div>

                <div className="store-utility-card">
                  <div className="card-header">
                    <div>
                      <span className="card-subtitle">YEARLY AVG</span>
                      <h3 className="card-title" style={{ fontSize: '24px', marginTop: '6px' }}>
                        {avgStats ? formatDuration(avgStats.yearly_avg_seconds) : '—'}
                      </h3>
                    </div>
                    <Activity size={18} color="var(--colors-primary)" />
                  </div>
                  <span style={{ fontSize: '13px', color: 'var(--colors-ink-muted-48)' }}>Average per year</span>
                </div>
              </div>
            </section>

            {/* Tile 3: Simplified app table */}
            <section className="viewport-tile parchment">
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '8px', flexWrap: 'wrap', gap: '16px' }}>
                <div>
                  <h2 className="hero-display" style={{ fontSize: '28px' }}>Application Directory</h2>
                  <p className="lead-subcopy" style={{ fontSize: '15px', marginTop: '4px' }}>All tracked applications and their screen time for {formattedSelectedDate}.</p>
                </div>
                <div style={{ width: '240px' }}>
                  <input
                    type="text"
                    placeholder="Search apps..."
                    value={searchTerm}
                    onChange={(e) => setSearchTerm(e.target.value)}
                    className="search-pill-input"
                    style={{ fontSize: '13.5px', padding: '8px 16px' }}
                  />
                </div>
              </div>

              <div className="store-utility-card" style={{ padding: '0px', overflow: 'hidden' }}>
                <table className="store-table">
                  <thead>
                    <tr>
                      <th>Executable</th>
                      <th>Display Name</th>
                      <th style={{ textAlign: 'right' }}>Usage</th>
                    </tr>
                  </thead>
                  <tbody>
                    {topApps.filter(app => app.executable_name.toLowerCase().includes(searchTerm.toLowerCase())).map(app => (
                      <tr key={app.executable_name}>
                        <td style={{ fontFamily: 'monospace', color: 'var(--colors-primary)' }}>{app.executable_name}</td>
                        <td style={{ fontWeight: 600 }}>{app.display_name}</td>
                        <td style={{ textAlign: 'right', fontWeight: 500 }}>{formatDuration(app.total_seconds)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </section>
          </>
        )}

        {activeTab === "settings" && (
          <>
            <section style={{ "height": "100%" }} className="viewport-tile light">
              <div>
                <h1 className="hero-display">System Settings</h1>
                <p className="lead-subcopy" style={{ marginTop: '8px' }}>Configure launch habits and database operations.</p>
              </div>

              <div className="store-utility-card" style={{ gap: '28px' }}>
                {/* Autostart toggle */}
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', borderBottom: '1px solid var(--colors-divider-soft)', paddingBottom: '20px' }}>
                  <div>
                    <h4 style={{ fontSize: '14px', fontWeight: 600 }}>Launch on Startup</h4>
                    <p className="lead-subcopy" style={{ fontSize: '12px', marginTop: '2px', color: 'var(--colors-ink-muted-48)' }}>Automatically start DigitalWellbeing Agent in the background when you log in.</p>
                  </div>
                  <div
                    className={`theme-toggle-track ${autostartEnabled ? 'active' : ''}`}
                    onClick={() => toggleAutostart(!autostartEnabled)}
                    style={{ cursor: 'pointer', flexShrink: 0 }}
                  >
                    <div className="theme-toggle-thumb" />
                  </div>
                </div>

                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', borderBottom: '1px solid var(--colors-divider-soft)', paddingBottom: '20px' }}>
                  <div>
                    <h4 style={{ fontSize: '14px', fontWeight: 600 }}>Idle Monitor Tracking</h4>
                    <p className="lead-subcopy" style={{ fontSize: '12px', marginTop: '2px', color: 'var(--colors-ink-muted-48)' }}>Pause telemetry automatically after 60 seconds of mouse/keyboard inactivity.</p>
                  </div>
                  <input
                    type="checkbox"
                    checked={idleMonitoring}
                    onChange={(e) => toggleIdleMonitoring(e.target.checked)}
                    style={{ width: '16px', height: '16px', accentColor: 'var(--colors-primary)', cursor: 'pointer' }}
                  />
                </div>

                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', borderBottom: '1px solid var(--colors-divider-soft)', paddingBottom: '20px' }}>
                  <div>
                    <h4 style={{ fontSize: '14px', fontWeight: 600 }}>Local-First Privacy</h4>
                    <p className="lead-subcopy" style={{ fontSize: '12px', marginTop: '2px', color: 'var(--colors-ink-muted-48)' }}>All logs remain strictly local. Export or audit your data.</p>
                  </div>
                  <div style={{ display: 'flex', gap: '8px' }}>
                    <button className="button-secondary-pill" onClick={() => exportTelemetry("csv")} style={{ padding: '6px 12px', fontSize: '11px' }}>Export CSV</button>
                    <button className="button-secondary-pill" onClick={() => exportTelemetry("json")} style={{ padding: '6px 12px', fontSize: '11px' }}>Export JSON</button>
                  </div>
                </div>

                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', borderBottom: '1px solid var(--colors-divider-soft)', paddingBottom: '20px' }}>
                  <div>
                    <h4 style={{ fontSize: '14px', fontWeight: 600 }}>Database Backup &amp; Restore</h4>
                    <p className="lead-subcopy" style={{ fontSize: '12px', marginTop: '2px', color: 'var(--colors-ink-muted-48)' }}>Save a copy of your database or restore from a previous backup. Restore will restart the app.</p>
                  </div>
                  <div style={{ display: 'flex', gap: '8px' }}>
                    <button
                      className="button-secondary-pill"
                      style={{ padding: '6px 12px', fontSize: '11px' }}
                      onClick={async () => {
                        try {
                          const path = await invoke<string>("backup_database");
                          alert(`Database backed up successfully to:\n${path}`);
                        } catch (err: any) {
                          if (err !== 'Backup cancelled') {
                            alert(`Backup failed: ${err}`);
                          }
                        }
                      }}
                    >
                      Backup DB
                    </button>
                    <button
                      className="button-secondary-pill"
                      style={{ padding: '6px 12px', fontSize: '11px', color: 'var(--colors-danger, #e05c5c)' }}
                      onClick={async () => {
                        const confirmed = window.confirm(
                          'Restoring a backup will REPLACE all current data and restart the app.\n\nAre you sure you want to continue?'
                        );
                        if (!confirmed) return;
                        try {
                          await invoke("restore_database");
                        } catch (err: any) {
                          if (err !== 'Restore cancelled') {
                            alert(`Restore failed: ${err}`);
                          }
                        }
                      }}
                    >
                      Restore DB
                    </button>
                  </div>
                </div>

                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', borderBottom: '1px solid var(--colors-divider-soft)', paddingBottom: '20px' }}>
                  <div>
                    <h4 style={{ fontSize: '14px', fontWeight: 600 }}>Project Resources</h4>
                    <p className="lead-subcopy" style={{ fontSize: '12px', marginTop: '2px', color: 'var(--colors-ink-muted-48)' }}>Access the open-source code and official website details.</p>
                  </div>
                  <div style={{ display: 'flex', gap: '8px' }}>
                    <button className="button-secondary-pill" onClick={() => openUrl("https://github.com/Harshshah6/digital-wellbeing")} style={{ padding: '6px 12px', fontSize: '11px' }}>GitHub URL</button>
                  </div>
                </div>

                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', borderBottom: '1px solid var(--colors-divider-soft)', paddingBottom: '20px' }}>
                  <div>
                    <h4 style={{ fontSize: '14px', fontWeight: 600 }}>Enterprise Agent</h4>
                    <p className="lead-subcopy" style={{ fontSize: '12px', marginTop: '2px', color: 'var(--colors-ink-muted-48)' }}>
                      This agent exposes a REST API on port <code style={{ fontFamily: 'monospace', color: 'var(--colors-primary)' }}>7842</code> and broadcasts its presence on UDP port <code style={{ fontFamily: 'monospace', color: 'var(--colors-primary)' }}>7843</code>. Open the Admin Dashboard on any machine in the same network to monitor all agents.
                    </p>
                  </div>
                  <div style={{ display: 'flex', gap: '8px' }}>
                    <button className="button-secondary-pill" onClick={() => openUrl("http://localhost:7842/api/info")} style={{ padding: '6px 12px', fontSize: '11px' }}>API Info</button>
                  </div>
                </div>

                <div>
                  <h4 style={{ fontSize: '13px', fontWeight: 600, color: 'var(--colors-ink-muted-48)', textTransform: 'uppercase', marginBottom: '8px' }}>Specification</h4>
                  <p className="lead-subcopy" style={{ fontSize: '12.5px', color: 'var(--colors-ink-muted-80)' }}>
                    DigitalWellbeing Enterprise Agent • <a target="_blank" href="https://github.com/Harshshah6/digital-wellbeing/releases/latest" style={{ color: 'var(--colors-ink-muted-80)' }}>Version {version}</a>
                  </p>
                </div>
              </div>
            </section>
          </>
        )}

      </div>
    </div >
  );
}
