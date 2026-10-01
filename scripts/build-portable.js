/**
 * build-portable.js
 * Creates a portable DigitalWellbeing distribution (cross-platform):
 * - Copies the agent binary into a self-contained folder
 * - Copies the admin-dashboard folder alongside
 * - Zips it as DigitalWellbeing-portable-v{version}.zip
 */

import { readFileSync, existsSync, rmSync, mkdirSync, copyFileSync, readdirSync, writeFileSync } from 'fs';
import { join } from 'path';
import { execSync } from 'child_process';
import os from 'os';

const isWindows = os.platform() === 'win32';
const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
const version = pkg.version;

// Resolve the platform-specific binary
const binarySrc = isWindows
  ? join('src-tauri', 'target', 'release', 'digital-wellbeing.exe')
  : join('src-tauri', 'target', 'release', 'digital-wellbeing');

const binaryDest = isWindows ? 'DigitalWellbeing.exe' : 'digital-wellbeing';

const portableDir = join('dist-portable', `DigitalWellbeing-v${version}`);
const agentDir = join(portableDir, 'Agent');
const dashboardDir = join(portableDir, 'AdminDashboard');

// Ensure output directories exist
mkdirSync(agentDir, { recursive: true });
mkdirSync(dashboardDir, { recursive: true });

// Copy agent binary
if (!existsSync(binarySrc)) {
  const buildCmd = isWindows ? '`npm run tauri:build-win`' : '`npm run tauri:build-linux`';
  console.error(`ERROR: Release binary not found at: ${binarySrc}`);
  console.error(`Run ${buildCmd} first.`);
  process.exit(1);
}
try {
  copyFileSync(binarySrc, join(agentDir, binaryDest));
  console.log(`✓ Copied ${binaryDest}`);
} catch (e) {
  if (e.code === 'EBUSY') {
    console.log(`ℹ ${binaryDest} is currently running; kept active binary in portable folder.`);
  } else {
    throw e;
  }
}

// Copy admin dashboard
const dashSrc = join('admin-dashboard');
readdirSync(dashSrc).forEach(f => {
  copyFileSync(join(dashSrc, f), join(dashboardDir, f));
});
console.log('✓ Copied Admin Dashboard');

// Write README
const firewallNote = isWindows
  ? `## Firewall (run once per agent PC as Administrator)\n  netsh advfirewall firewall add rule name="DigitalWellbeing API" dir=in action=allow protocol=TCP localport=7842`
  : `## Firewall (run once per agent machine)\n  sudo ufw allow 7842/tcp\n  sudo ufw allow 7843/udp`;

const agentRunNote = isWindows
  ? `Run \`DigitalWellbeing.exe\` — it will start minimised to the system tray`
  : `Make it executable and run: \`chmod +x digital-wellbeing && ./digital-wellbeing\``;

const readme = `# DigitalWellbeing Enterprise v${version}

## Agent (Employee PCs)
1. Copy the \`Agent\` folder to the employee's PC
2. ${agentRunNote}
3. The agent exposes its REST API on port 7842 and broadcasts on UDP 7843

## Admin Dashboard
1. Open \`AdminDashboard/index.html\` in any modern browser (Chrome/Edge/Firefox)
2. The dashboard will auto-scan the LAN for agents
3. Or manually enter an agent IP in the input field

${firewallNote}
`;
writeFileSync(join(portableDir, 'README.txt'), readme);
console.log('✓ Created README.txt');

// Zip — use zip on Unix, PowerShell on Windows
const zipName = `DigitalWellbeing-portable-v${version}.zip`;
const zipOut = join('dist-portable', zipName);
try {
  if (isWindows) {
    execSync(
      `powershell -Command "Compress-Archive -Path '${portableDir}' -DestinationPath '${zipOut}' -Force"`,
      { stdio: 'inherit' }
    );
  } else {
    execSync(`zip -r "${zipOut}" "${portableDir}"`, { stdio: 'inherit' });
  }
  console.log(`\n✅ Portable package: dist-portable/${zipName}`);
} catch (e) {
  console.log('\n⚠ Zip failed — portable folder is at: dist-portable/');
}

// Show final output
const installerPath = isWindows
  ? `src-tauri\\target\\release\\bundle\\nsis\\DigitalWellbeing_${version}_x64-setup.exe`
  : `src-tauri/target/release/bundle/deb/digital-wellbeing_${version}_amd64.deb`;

console.log('\n📦 Output:');
console.log(`  Installer: ${installerPath}`);
console.log(`  Portable:  dist-portable/${zipName}`);
