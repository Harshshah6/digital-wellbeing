# DigitalWellbeing Enterprise

> **Windows-only** enterprise employee digital wellbeing monitoring system.

## Architecture

```
┌─────────────────────────────────────┐    LAN
│   Employee PC (Windows)             │
│   ┌────────────────────────────┐    │
│   │  DigitalWellbeing Agent    │◄───┼── System Tray (background)
│   │  (Tauri + Rust)            │    │
│   │  ┌──────────────────────┐  │    │
│   │  │  SQLite DB           │  │    │
│   │  │  (local, per-device) │  │    │
│   │  └──────────────────────┘  │    │
│   │  ┌──────────────────────┐  │    │
│   │  │  HTTP REST API       │  │◄───┼── Admin Dashboard polls
│   │  │  port 7842           │  │    │   http://<ip>:7842/api/...
│   │  └──────────────────────┘  │    │
│   │  ┌──────────────────────┐  │    │
│   │  │  UDP Beacon          │  │──► │── broadcasts every 5s
│   │  │  port 7843           │  │    │   {hostname, ip, device_id}
│   │  └──────────────────────┘  │    │
│   └────────────────────────────┘    │
└─────────────────────────────────────┘
         ▲
         │ HTTP GET (probes every 8s)
         │
┌─────────────────────────────────────┐
│   Admin's Browser (any OS)          │
│   admin-dashboard/index.html        │
│   (standalone HTML — no install)    │
└─────────────────────────────────────┘
```

## Quick Start

### 1. Deploy Agent on Employee PCs

Install/run `DigitalWellbeing.exe` on each Windows PC. The agent will:
- Track active application usage every second
- Store data locally in SQLite at `%LOCALAPPDATA%\com.digitalwellbeing.agent\wellbeing.db`
- Start automatically on Windows login (configurable in Settings)
- **Expose REST API on `http://0.0.0.0:7842`** (LAN-accessible)
- **Broadcast UDP beacon to subnet on port 7843** every 5 seconds

### 2. Open Admin Dashboard

Open `admin-dashboard/index.html` in any modern browser (Chrome, Edge, Firefox).

The dashboard will:
1. **Auto-scan** common LAN subnets (`192.168.1.x`, `192.168.0.x`, `10.0.0.x`, etc.)
2. **Detect** any machine running the DigitalWellbeing agent
3. **Display** live fleet overview (online count, avg screen time)
4. **Allow drilling** into any device for full detail

You can also enter an IP address or subnet manually (e.g., `192.168.2` to scan all 192.168.2.x hosts).

## API Reference

All endpoints are served on `http://<agent-ip>:7842`. CORS is fully open so the dashboard can query from any origin.

| Endpoint | Method | Query Params | Description |
|---|---|---|---|
| `/api/info` | GET | — | Agent metadata (hostname, IP, device ID, version) |
| `/api/top_apps` | GET | `date=YYYY-MM-DD&limit=10` | Top apps by screen time |
| `/api/categories` | GET | `date=YYYY-MM-DD` | Category breakdown |
| `/api/timeline` | GET | `date=YYYY-MM-DD` | Hourly activity (24 buckets) |
| `/api/heatmap` | GET | — | All-time daily screen time totals |
| `/api/avg_stats` | GET | — | Daily/weekly/monthly/yearly averages |

### Example

```bash
# Get today's top apps from device at 192.168.1.105
curl http://192.168.1.105:7842/api/top_apps?date=2026-09-27&limit=5

# Get agent info
curl http://192.168.1.105:7842/api/info
```

## Network Requirements

| Port | Protocol | Direction | Purpose |
|---|---|---|---|
| `7842` | TCP/HTTP | Inbound on agent PCs | REST API for admin dashboard |
| `7843` | UDP/Broadcast | Outbound from agent PCs | Discovery beacon |

> **Firewall note:** Ensure port 7842 is allowed for inbound TCP on the agent PCs in Windows Defender Firewall. You can add a rule via:
> ```powershell
> netsh advfirewall firewall add rule name="DigitalWellbeing API" dir=in action=allow protocol=TCP localport=7842
> ```

## Building

```powershell
# Development
npm run tauri:dev

# Production (Windows installer: .msi + .exe)
npm run tauri:build-win
```

## Data Privacy

- All data is stored **locally on each device** in SQLite
- The REST API exposes read-only endpoints — no write access from the network
- The admin dashboard **never uploads** any data to external servers
- Suitable for internal corporate networks
