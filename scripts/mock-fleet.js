/**
 * mock-fleet.js
 * Simulates multiple DigitalWellbeing employee agent devices on local ports.
 * Run with: node scripts/mock-fleet.js
 * Press Ctrl+C to stop.
 */

import http from 'http';

const MOCK_DEVICES = [
  {
    port: 7851,
    hostname: 'FINANCE-DESK-01',
    device_id: 'dw-mock-fin-8841',
    user: 'Sarah M. (Finance)',
    screenTime: 24120, // 6h 42m
    dailyAvg: 23400,
    weeklyAvg: 117000,
    apps: [
      { display_name: 'Microsoft Excel', executable_name: 'excel.exe', category: 'Productivity', total_seconds: 14200 },
      { display_name: 'SAP GUI', executable_name: 'saplogon.exe', category: 'Business', total_seconds: 5400 },
      { display_name: 'Microsoft Outlook', executable_name: 'outlook.exe', category: 'Communication', total_seconds: 2800 },
      { display_name: 'Google Chrome', executable_name: 'chrome.exe', category: 'Browsing', total_seconds: 1720 }
    ],
    categories: [
      { category: 'Productivity', total_seconds: 14200 },
      { category: 'Business', total_seconds: 5400 },
      { category: 'Communication', total_seconds: 2800 },
      { category: 'Browsing', total_seconds: 1720 }
    ],
    timelineHours: [0,0,0,0,0,0,0,0,900,3200,3600,3400,1800,2900,3500,3400,2300,1120,0,0,0,0,0,0]
  },
  {
    port: 7852,
    hostname: 'DEV-WORKSTATION-03',
    device_id: 'dw-mock-dev-1928',
    user: 'Alex K. (Engineering)',
    screenTime: 28680, // 7h 58m
    dailyAvg: 27000,
    weeklyAvg: 135000,
    apps: [
      { display_name: 'Visual Studio Code', executable_name: 'code.exe', category: 'Development', total_seconds: 17400 },
      { display_name: 'Windows Terminal', executable_name: 'windowsterminal.exe', category: 'Development', total_seconds: 5100 },
      { display_name: 'Google Chrome', executable_name: 'chrome.exe', category: 'Browsing', total_seconds: 4180 },
      { display_name: 'Slack', executable_name: 'slack.exe', category: 'Communication', total_seconds: 2000 }
    ],
    categories: [
      { category: 'Development', total_seconds: 22500 },
      { category: 'Browsing', total_seconds: 4180 },
      { category: 'Communication', total_seconds: 2000 }
    ],
    timelineHours: [0,0,0,0,0,0,0,0,1200,3600,3600,3600,2400,3500,3600,3500,3400,2680,1200,0,0,0,0,0]
  },
  {
    port: 7853,
    hostname: 'SALES-LAPTOP-02',
    device_id: 'dw-mock-sal-5531',
    user: 'Marcus T. (Sales)',
    screenTime: 18900, // 5h 15m
    dailyAvg: 19500,
    weeklyAvg: 97500,
    apps: [
      { display_name: 'Zoom Workplace', executable_name: 'zoom.exe', category: 'Communication', total_seconds: 8200 },
      { display_name: 'Salesforce (Chrome)', executable_name: 'chrome.exe', category: 'Business', total_seconds: 6100 },
      { display_name: 'Microsoft Outlook', executable_name: 'outlook.exe', category: 'Communication', total_seconds: 3400 },
      { display_name: 'Notion', executable_name: 'notion.exe', category: 'Productivity', total_seconds: 1200 }
    ],
    categories: [
      { category: 'Communication', total_seconds: 11600 },
      { category: 'Business', total_seconds: 6100 },
      { category: 'Productivity', total_seconds: 1200 }
    ],
    timelineHours: [0,0,0,0,0,0,0,0,600,2800,3400,3200,1200,2400,3100,2200,0,0,0,0,0,0,0,0]
  },
  {
    port: 7854,
    hostname: 'MARKETING-RIG-05',
    device_id: 'dw-mock-mkt-7729',
    user: 'Elena R. (Design)',
    screenTime: 22200, // 6h 10m
    dailyAvg: 21600,
    weeklyAvg: 108000,
    apps: [
      { display_name: 'Adobe Photoshop 2026', executable_name: 'photoshop.exe', category: 'Design', total_seconds: 11000 },
      { display_name: 'Figma', executable_name: 'figma.exe', category: 'Design', total_seconds: 6800 },
      { display_name: 'Spotify', executable_name: 'spotify.exe', category: 'Entertainment', total_seconds: 2400 },
      { display_name: 'Slack', executable_name: 'slack.exe', category: 'Communication', total_seconds: 2000 }
    ],
    categories: [
      { category: 'Design', total_seconds: 17800 },
      { category: 'Entertainment', total_seconds: 2400 },
      { category: 'Communication', total_seconds: 2000 }
    ],
    timelineHours: [0,0,0,0,0,0,0,0,1100,3300,3600,3400,1800,3100,3400,2500,0,0,0,0,0,0,0,0]
  }
];

function generateHeatmap(baseSeconds) {
  const result = [];
  const now = new Date();
  for (let i = 119; i >= 0; i--) {
    const d = new Date(now);
    d.setDate(now.getDate() - i);
    const dayOfWeek = d.getDay();
    const isWeekend = dayOfWeek === 0 || dayOfWeek === 6;
    const factor = isWeekend ? (Math.random() < 0.2 ? 0.3 : 0) : (0.7 + Math.random() * 0.5);
    const seconds = Math.round(baseSeconds * factor);
    result.push({
      date: d.toISOString().split('T')[0],
      count: seconds
    });
  }
  return result;
}

const servers = [];

MOCK_DEVICES.forEach(dev => {
  const server = http.createServer((req, res) => {
    res.setHeader('Access-Control-Allow-Origin', '*');
    res.setHeader('Access-Control-Allow-Methods', 'GET, OPTIONS');
    res.setHeader('Access-Control-Allow-Headers', 'Content-Type');

    if (req.method === 'OPTIONS') {
      res.writeHead(204);
      res.end();
      return;
    }

    res.setHeader('Content-Type', 'application/json');

    const url = new URL(req.url, `http://localhost:${dev.port}`);
    const pathname = url.pathname;

    if (pathname === '/api/info') {
      res.end(JSON.stringify({
        app: 'DigitalWellbeing',
        version: '1.0.0',
        hostname: dev.hostname,
        device_id: dev.device_id,
        user: dev.user,
        port: dev.port
      }));
    } else if (pathname === '/api/top_apps') {
      res.end(JSON.stringify(dev.apps));
    } else if (pathname === '/api/categories') {
      res.end(JSON.stringify(dev.categories));
    } else if (pathname === '/api/timeline') {
      const data = dev.timelineHours.map((total_seconds, hour) => ({ hour, total_seconds }));
      res.end(JSON.stringify(data));
    } else if (pathname === '/api/avg_stats') {
      res.end(JSON.stringify({
        daily_avg_seconds: dev.dailyAvg,
        weekly_avg_seconds: dev.weeklyAvg,
        monthly_avg_seconds: dev.dailyAvg * 22,
        yearly_avg_seconds: dev.dailyAvg * 250
      }));
    } else if (pathname === '/api/heatmap') {
      res.end(JSON.stringify(generateHeatmap(dev.screenTime)));
    } else {
      res.writeHead(404);
      res.end(JSON.stringify({ error: 'Not found' }));
    }
  });

  server.listen(dev.port, '0.0.0.0', () => {
    console.log(`  🖥️  ${dev.hostname} (${dev.user}) -> http://localhost:${dev.port}`);
  });

  servers.push(server);
});

console.log('\n🚀 DigitalWellbeing Mock Fleet Running!');
console.log('Open admin-dashboard/index.html and click "Scan Network" to discover all simulated devices.\n');

process.on('SIGINT', () => {
  console.log('\nStopping mock fleet...');
  servers.forEach(s => s.close());
  process.exit(0);
});
