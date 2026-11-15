#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, '..');
const checkOnly = process.argv.includes('--check');

function readJson(relPath) {
  return JSON.parse(fs.readFileSync(path.join(repoRoot, relPath), 'utf8'));
}

function readText(relPath) {
  return fs.readFileSync(path.join(repoRoot, relPath), 'utf8');
}

function extractMcpTools() {
  const src = readText('crates/psalm-stage-cli/src/mcp.rs');
  const fnMatch = src.match(/pub fn mcp_tool_definitions\(\)[\s\S]*?\n\}/);
  if (!fnMatch) {
    throw new Error('Could not locate mcp_tool_definitions() in crates/psalm-stage-cli/src/mcp.rs');
  }
  const block = fnMatch[0];
  const toolRegex = /"name":\s*"([^"]+)",\s*\n\s*"description":\s*"([^"]+)"/g;
  const tools = [];
  let match;
  while ((match = toolRegex.exec(block)) !== null) {
    tools.push({ name: match[1], description: match[2] });
  }
  if (tools.length === 0) {
    throw new Error('Extracted 0 MCP tools from mcp_tool_definitions()');
  }
  return tools;
}

function extractCliUsage() {
  const src = readText('crates/psalm-stage-cli/src/main.rs');
  const match = src.match(/fn print_help\(\)\s*\{\s*eprintln!\(\s*"([\s\S]*?)"\s*\);\s*\}/);
  if (!match) {
    throw new Error('Could not locate print_help() in crates/psalm-stage-cli/src/main.rs');
  }
  return match[1]
    .replace(/\\\n\s*/g, '')
    .replace(/\\n/g, '\n')
    .replace(/\\t/g, '  ')
    .trim();
}

function buildInstallersMarkdown(version) {
  return [
    '| Platform | Installer Download | Installation Steps |',
    '| :--- | :--- | :--- |',
    `| **macOS (Apple Silicon)** | \`Psalm.Stage_${version}_aarch64.dmg\` | Open the \`.dmg\` file and drag **Psalm Stage** into your **Applications** folder. |`,
    `| **macOS (Intel Core)** | \`Psalm.Stage_${version}_x64.dmg\` | Open the \`.dmg\` file and drag **Psalm Stage** into your **Applications** folder. |`,
    `| **Windows 10 / 11** | \`Psalm.Stage_${version}_x64-setup.exe\` | Double-click the \`.exe\` installer. A desktop shortcut and Start menu entry are created automatically. |`,
    `| **Linux** | \`Psalm.Stage_${version}_amd64.AppImage\` | Right-click the \`.AppImage\`, enable "Allow executing file as program", and double-click to launch. |`,
  ].join('\n');
}

function buildWebsiteVersionNote(version) {
  return `Current release: **\`v${version}\`** — installers on [GitHub Releases](https://github.com/adedayo/psalm-stage/releases/latest) include \`Psalm.Stage_${version}_aarch64.dmg\`, \`Psalm.Stage_${version}_x64.dmg\`, \`Psalm.Stage_${version}_x64-setup.exe\`, and \`Psalm.Stage_${version}_amd64.AppImage\`.`;
}

function buildMcpToolsMarkdown(tools) {
  const lines = [
    '| MCP Tool | Purpose |',
    '| :--- | :--- |',
  ];
  for (const tool of tools) {
    lines.push(`| \`${tool.name}\` | ${tool.description} |`);
  }
  return lines.join('\n');
}

function buildCliHelpMarkdown(cliHelp) {
  return ['```text', cliHelp, '```'].join('\n');
}

function replaceBlock(content, blockId, replacement, isMdx) {
  const startMarker = isMdx ? `{/* AUTO-SYNC:START ${blockId} */}` : `<!-- AUTO-SYNC:START ${blockId} -->`;
  const endMarker = isMdx ? `{/* AUTO-SYNC:END ${blockId} */}` : `<!-- AUTO-SYNC:END ${blockId} -->`;
  const startIdx = content.indexOf(startMarker);
  const endIdx = content.indexOf(endMarker);
  if (startIdx === -1 || endIdx === -1 || endIdx < startIdx) {
    throw new Error(`Missing or malformed AUTO-SYNC block "${blockId}"`);
  }
  const before = content.slice(0, startIdx + startMarker.length);
  const after = content.slice(endIdx);
  return `${before}\n${replacement}\n${after}`;
}

function main() {
  const pkg = readJson('package.json');
  const version = pkg.version;
  const mcpTools = extractMcpTools();
  const cliHelp = extractCliUsage();

  const targets = [
    {
      path: 'README.md',
      updates: [['installers-table', buildInstallersMarkdown(version)]],
    },
    {
      path: 'website/src/content/docs/getting-started/installation.mdx',
      updates: [['latest-release-note', buildWebsiteVersionNote(version)]],
    },
    {
      path: 'website/src/content/docs/workbench/ai-automation.mdx',
      updates: [
        ['mcp-tools-table', buildMcpToolsMarkdown(mcpTools)],
        ['cli-usage', buildCliHelpMarkdown(cliHelp)],
      ],
    },
  ];

  let outOfSync = 0;
  for (const target of targets) {
    const absPath = path.join(repoRoot, target.path);
    const original = fs.readFileSync(absPath, 'utf8');
    const isMdx = target.path.endsWith('.mdx');
    let next = original;
    for (const [blockId, replacement] of target.updates) {
      next = replaceBlock(next, blockId, replacement, isMdx);
    }
    if (next !== original) {
      if (checkOnly) {
        console.error(`OUT OF SYNC: ${target.path}`);
        outOfSync += 1;
      } else {
        fs.writeFileSync(absPath, next, 'utf8');
        console.log(`==> Synced ${target.path}`);
      }
    } else {
      console.log(`==> Up to date: ${target.path}`);
    }
  }

  if (checkOnly && outOfSync > 0) {
    console.error(`\nRun 'node scripts/sync-docs.mjs' to update ${outOfSync} out-of-sync documentation file(s).`);
    process.exit(1);
  }
}

main();
