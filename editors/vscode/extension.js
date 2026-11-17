// VS Code extension for Meng: completion, hover, signature help, go-to-definition,
// live diagnostics (via `meng check -`), and run commands.

const vscode = require('vscode');
const cp = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');
const {
  KEYWORDS, TYPES, CONSTANTS, BUILTINS,
  indexSymbols, fromCompiler, visibleVariables, callContext, describeVar,
} = require('./symbols');

const LANG = 'meng';
const DIAGNOSTIC_DELAY_MS = 300;

// VS Code launched from the Dock often lacks the install folders in PATH, so look
// there too: ~/.meng/bin (prebuilt download) and ~/.cargo/bin (built from source).
const INSTALLED = [
  path.join(os.homedir(), '.meng', 'bin', 'meng'),
  path.join(os.homedir(), '.cargo', 'bin', 'meng'),
];

function compilerPath() {
  const configured = vscode.workspace.getConfiguration('meng').get('compilerPath') || 'meng';
  if (configured !== 'meng') return configured;
  return INSTALLED.find((p) => fs.existsSync(p)) || configured;
}

/** The document's folder: `use "x.meng";` resolves from there. */
function workingDir(document) {
  return path.dirname(document.uri.fsPath);
}

// ---------------------------------------------------------------- symbols

/** Last `meng symbols` result per document: { version, promise }. */
const symbolCache = new Map();

/** Ask the compiler for symbols (null if it is missing or too old). */
function compilerSymbols(document) {
  const key = document.uri.toString();
  const hit = symbolCache.get(key);
  if (hit && hit.version === document.version) return hit.promise;
  const promise = new Promise((resolve) => {
    const child = cp.spawn(compilerPath(), ['symbols', '-'], { cwd: workingDir(document) });
    let out = '';
    child.stdout.on('data', (d) => (out += d));
    child.on('error', () => resolve(null));
    child.on('close', () => {
      try {
        resolve(fromCompiler(JSON.parse(out)));
      } catch {
        resolve(null);
      }
    });
    child.stdin.on('error', () => {});
    child.stdin.end(document.getText());
  });
  symbolCache.set(key, { version: document.version, promise });
  return promise;
}

/** Compiler symbols when available, otherwise the built-in regex index. */
async function indexFor(document) {
  return (await compilerSymbols(document)) || indexSymbols(document.getText());
}

function wordAt(document, position) {
  const range = document.getWordRangeAtPosition(position, /[A-Za-z_][A-Za-z0-9_]*/);
  return range ? document.getText(range) : null;
}

// ---------------------------------------------------------------- completion

const completionProvider = {
  async provideCompletionItems(document, position) {
    const prefix = document.lineAt(position).text.slice(0, position.character);

    // After `name:` only types make sense.
    if (/:\s*[A-Za-z_]*$/.test(prefix)) {
      return TYPES.map((t) => new vscode.CompletionItem(t, vscode.CompletionItemKind.TypeParameter));
    }

    const index = await indexFor(document);
    const items = [];

    for (const k of KEYWORDS) items.push(new vscode.CompletionItem(k, vscode.CompletionItemKind.Keyword));
    for (const c of CONSTANTS) items.push(new vscode.CompletionItem(c, vscode.CompletionItemKind.Constant));
    for (const b of BUILTINS) {
      const item = new vscode.CompletionItem(b.name + '()', vscode.CompletionItemKind.Function);
      item.detail = `built-in: ${b.signature}`;
      item.filterText = b.name;
      item.insertText = new vscode.SnippetString(b.snippet);
      items.push(item);
    }

    for (const f of index.functions) {
      const item = new vscode.CompletionItem(f.name, vscode.CompletionItemKind.Function);
      item.detail = f.signature;
      const required = f.params.filter((p) => !p.default);
      const args = required.map((p, i) => `\${${i + 1}:${p.name}}`).join(', ');
      item.insertText = new vscode.SnippetString(`${f.name}(${args});`);
      items.push(item);
    }

    for (const v of visibleVariables(index, position.line)) {
      const kind = v.kind === 'keep' ? vscode.CompletionItemKind.Constant : vscode.CompletionItemKind.Variable;
      const item = new vscode.CompletionItem(v.name, kind);
      item.detail = describeVar(v);
      items.push(item);
    }
    return items;
  },
};

// ---------------------------------------------------------------- hover

const hoverProvider = {
  async provideHover(document, position) {
    const word = wordAt(document, position);
    if (!word) return null;
    const index = await indexFor(document);

    const f = index.functions.find((fn) => fn.name === word);
    if (f) return new vscode.Hover(new vscode.MarkdownString().appendCodeblock(f.signature, LANG));

    const v = visibleVariables(index, position.line, true).find((x) => x.name === word);
    if (v) return new vscode.Hover(new vscode.MarkdownString().appendCodeblock(describeVar(v), LANG));
    return null;
  },
};

// ---------------------------------------------------------------- signature help

const signatureProvider = {
  async provideSignatureHelp(document, position) {
    const prefix = document.lineAt(position).text.slice(0, position.character);
    const ctx = callContext(prefix);
    if (!ctx) return null;
    const f = (await indexFor(document)).functions.find((fn) => fn.name === ctx.name);
    if (!f) return null;

    const sig = new vscode.SignatureInformation(f.signature);
    sig.parameters = f.params.map((p) => {
      const label = `${p.kind === 'keep' || p.kind === 'reuse' ? p.kind + ' ' : ''}${p.name}: ${p.type}`;
      return new vscode.ParameterInformation(p.default ? `${label} = ${p.default}` : label);
    });
    const help = new vscode.SignatureHelp();
    help.signatures = [sig];
    help.activeSignature = 0;
    help.activeParameter = Math.min(ctx.argIndex, Math.max(f.params.length - 1, 0));
    return help;
  },
};

// ---------------------------------------------------------------- go to definition

const definitionProvider = {
  async provideDefinition(document, position) {
    const word = wordAt(document, position);
    if (!word) return null;
    const index = await indexFor(document);
    const target =
      visibleVariables(index, position.line, true).find((v) => v.name === word) ||
      index.functions.find((f) => f.name === word);
    if (!target) return null;
    const uri = target.file ? vscode.Uri.file(path.resolve(workingDir(document), target.file)) : document.uri;
    return new vscode.Location(uri, new vscode.Position(target.line, Math.max(target.column, 0)));
  },
};

// ---------------------------------------------------------------- diagnostics

let warnedMissingCompiler = false;

/** Run `meng check -` on the document text and show its errors. */
function checkDocument(document, collection) {
  if (document.languageId !== LANG) return;
  const child = cp.spawn(compilerPath(), ['check', '-'], { cwd: workingDir(document) });
  let stderr = '';
  child.stderr.on('data', (d) => (stderr += d));

  child.on('error', (err) => {
    collection.delete(document.uri);
    if (err.code === 'ENOENT' && !warnedMissingCompiler) {
      warnedMissingCompiler = true;
      vscode.window.showWarningMessage(
        `Meng compiler not found ("${compilerPath()}"). Set "meng.compilerPath" to enable diagnostics.`,
      );
    }
  });

  child.on('close', () => {
    const diagnostics = [];
    // `[ERROR] -:line:column: message` for this document (errors inside imported
    // files are named after those files; their `use` line gets its own error).
    for (const m of stderr.matchAll(/^\[ERROR\] -:(\d+):(\d+): (.*)$/gm)) {
      const line = Math.min(Number(m[1]) - 1, document.lineCount - 1);
      const text = document.lineAt(line).text;
      const column = Math.min(Number(m[2]) - 1, text.length);
      // Underline the word at the column (or the rest of the line).
      const word = document.getWordRangeAtPosition(new vscode.Position(line, column), /[A-Za-z_][A-Za-z0-9_]*|\S+/);
      const range = word || new vscode.Range(line, column, line, text.length);
      const diagnostic = new vscode.Diagnostic(range, m[3], vscode.DiagnosticSeverity.Error);
      diagnostic.source = 'meng';
      diagnostics.push(diagnostic);
    }
    collection.set(document.uri, diagnostics);
  });

  child.stdin.on('error', () => {}); // compiler missing or exited early
  child.stdin.end(document.getText());
}

// ---------------------------------------------------------------- commands

function runInTerminal(subcommand) {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.languageId !== LANG) {
    vscode.window.showErrorMessage('Open a Meng file first.');
    return;
  }
  editor.document.save().then(() => {
    const file = editor.document.uri.fsPath;
    const terminal = vscode.window.terminals.find((t) => t.name === 'Meng') || vscode.window.createTerminal('Meng');
    terminal.show();
    terminal.sendText(`cd "${path.dirname(file)}" && "${compilerPath()}" ${subcommand} "${file}"`);
  });
}

// ---------------------------------------------------------------- activation

function activate(context) {
  const selector = { language: LANG };
  const collection = vscode.languages.createDiagnosticCollection(LANG);
  const timers = new Map();

  const scheduleCheck = (document) => {
    if (document.languageId !== LANG) return;
    const key = document.uri.toString();
    clearTimeout(timers.get(key));
    timers.set(key, setTimeout(() => checkDocument(document, collection), DIAGNOSTIC_DELAY_MS));
  };

  context.subscriptions.push(
    collection,
    vscode.languages.registerCompletionItemProvider(selector, completionProvider, ':'),
    vscode.languages.registerHoverProvider(selector, hoverProvider),
    vscode.languages.registerSignatureHelpProvider(selector, signatureProvider, '(', ','),
    vscode.languages.registerDefinitionProvider(selector, definitionProvider),
    vscode.workspace.onDidOpenTextDocument(scheduleCheck),
    vscode.workspace.onDidChangeTextDocument((e) => scheduleCheck(e.document)),
    vscode.workspace.onDidCloseTextDocument((d) => {
      collection.delete(d.uri);
      symbolCache.delete(d.uri.toString());
    }),
    vscode.commands.registerCommand('meng.interpret', () => runInTerminal('interpret')),
    vscode.commands.registerCommand('meng.run', () => runInTerminal('run')),
  );

  vscode.workspace.textDocuments.forEach(scheduleCheck);
}

function deactivate() {}

module.exports = { activate, deactivate };
