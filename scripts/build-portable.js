/**
 * build-portable.js
 * Creates a portable DigitalWellbeing distribution:
 * - Copies digital-wellbeing.exe into a self-contained folder
 * - Copies the admin-dashboard folder alongside
 * - Zips it as DigitalWellbeing-portable-v{version}.zip
 */

import { readFileSync, existsSync, rmSync, mkdirSync, copyFileSync, readdirSync, writeFileSync } from 'fs';
import { join } from 'path';
import { execSync } from 'child_process';

const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
const version = pkg.version;

const exeSrc = join('src-tauri', 'target', 'release', 'digital-wellbeing.exe');
const portableDir = join('dist-portable', `DigitalWellbeing-v${version}`);
const agentDir = join(portableDir, 'Agent');
const dashboardDir = join(portableDir, 'AdminDashboard');

// Ensure output directories exist
mkdirSync(agentDir, { recursive: true });
mkdirSync(dashboardDir, { recursive: true });

// Copy exe
if (!existsSync(exeSrc)) {
  console.error('ERROR: Release exe not found. Run `npm run tauri:build-win` first.');
  process.exit(1);
}
try {
  copyFileSync(exeSrc, join(agentDir, 'DigitalWellbeing.exe'));
  console.log('✓ Copied DigitalWellbeing.exe');
} catch (e) {
  if (e.code === 'EBUSY') {
    console.log('ℹ DigitalWellbeing.exe is currently running; kept active binary in portable folder.');
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
const readme = `# DigitalWellbeing Enterprise v${version}

## Agent (Employee PCs)
1. Copy the \`Agent\` folder to the employee's Windows PC
2. Run \`DigitalWellbeing.exe\` — it will start minimised to system tray
3. The agent exposes its REST API on port 7842 and broadcasts on UDP 7843

## Admin Dashboard
1. Open \`AdminDashboard\\index.html\` in any modern browser (Chrome/Edge/Firefox)
2. The dashboard will auto-scan the LAN for agents
3. Or manually enter an agent IP in the input field

## Firewall (run once per agent PC as Administrator)
  netsh advfirewall firewall add rule name="DigitalWellbeing API" dir=in action=allow protocol=TCP localport=7842
`;
writeFileSync(join(portableDir, 'README.txt'), readme);
console.log('✓ Created README.txt');

// Zip with PowerShell
const zipName = `DigitalWellbeing-portable-v${version}.zip`;
const zipOut = join('dist-portable', zipName);
try {
  execSync(
    `powershell -Command "Compress-Archive -Path '${portableDir}' -DestinationPath '${zipOut}' -Force"`,
    { stdio: 'inherit' }
  );
  console.log(`\n✅ Portable package: dist-portable\\${zipName}`);
} catch (e) {
  console.log('\n⚠ Zip failed — portable folder is at: dist-portable\\');
}

// Show final output
console.log('\n📦 Output:');
console.log(`  Installer: src-tauri\\target\\release\\bundle\\nsis\\DigitalWellbeing_${version}_x64-setup.exe`);
console.log(`  Portable:  dist-portable\\${zipName}`);
