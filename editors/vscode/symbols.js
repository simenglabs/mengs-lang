// Lightweight symbol index for Meng source (functions, parameters, variables).
// Pure JavaScript with no VS Code dependency, so it can be tested with plain Node.
// Type checking and errors come from the real compiler (`meng check`); this file
// only powers completion, hover, signature help, and go-to-definition.

const KEYWORDS = ['set', 'keep', 'reuse', 'show', 'fnc', 'if', 'else', 'while', 'return', 'and', 'or', 'not', 'mod'];
const TYPES = ['str', 'int', 'float', 'bool'];
const CONSTANTS = ['true', 'false'];
const BUILTINS = [
  { name: 'str', signature: 'str(value): str', snippet: 'str(${1:value})' },
  { name: 'int', signature: 'int(value): int', snippet: 'int(${1:value})' },
  { name: 'float', signature: 'float(value): float', snippet: 'float(${1:value})' },
  { name: 'read', signature: 'read(prompt?): str', snippet: 'read(${1})' },
  { name: 'len', signature: 'len(array or str): int', snippet: 'len(${1:items})' },
  { name: 'push', signature: 'push(array, value);', snippet: 'push(${1:items}, ${2:value});' },
];

const IDENT = '[A-Za-z_][A-Za-z0-9_]*';
const FNC_RE = new RegExp(`^(\\s*fnc\\s+)(${IDENT})\\s*\\((.*)\\)\\s*(?::\\s*(${IDENT})\\s*)?->\\s*$`);
const SET_RE = new RegExp(`^(\\s*set\\s+(?:(keep|reuse)\\s+)?)(${IDENT})\\s*:\\s*(${IDENT})\\s*(<==|=)`);
const COPY_RE = new RegExp(`^\\s*(${IDENT})\\s*\\(\\)=>\\s*(${IDENT})\\s*;`);
const PARAM_RE = new RegExp(`^(?:(keep|reuse)\\s+)?(${IDENT})\\s*:\\s*(${IDENT})\\s*(?:=\\s*(.+))?$`);

/** Replace `% ... %` comments with spaces, keeping line/column positions. */
function stripComments(text) {
  let out = '';
  let inComment = false, inStr = false, escaped = false;
  for (const c of text) {
    if (c === '\n') {
      inStr = false; escaped = false;
      out += c;
      continue;
    }
    if (inComment) {
      if (c === '%') inComment = false;
      out += ' ';
      continue;
    }
    if (inStr) {
      if (escaped) escaped = false;
      else if (c === '\\') escaped = true;
      else if (c === '"') inStr = false;
    } else if (c === '"') {
      inStr = true;
    } else if (c === '%') {
      inComment = true;
      out += ' ';
      continue;
    }
    out += c;
  }
  return out;
}

/** Split on commas outside of string literals. */
function splitArgs(s) {
  const parts = [];
  let start = 0, inStr = false, escaped = false;
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (inStr) {
      if (escaped) escaped = false;
      else if (c === '\\') escaped = true;
      else if (c === '"') inStr = false;
    } else if (c === '"') {
      inStr = true;
    } else if (c === ',') {
      parts.push(s.slice(start, i));
      start = i + 1;
    }
  }
  parts.push(s.slice(start));
  return parts.map((p) => p.trim()).filter((p) => p.length > 0);
}

function describeVar(v) {
  const prefix = { keep: 'keep ', reuse: 'reuse ' }[v.kind] || '';
  const note = { borrow: ' (borrowed)', copy: ' (copy)', param: ' (parameter)' }[v.kind] || '';
  const moved = v.movedAt !== null && v.movedAt !== undefined ? ` (moved on line ${v.movedAt + 1})` : '';
  return `${prefix}${v.name}: ${v.type}${note}${moved}`;
}

/**
 * Index a Meng document.
 * Returns { functions, variables, scopeAt(line) } where `scope` is the enclosing
 * function name, or null for the top level.
 */
function indexSymbols(text) {
  const lines = stripComments(text).split(/\r?\n/);
  const functions = [];
  const variables = [];
  let current = null; // function currently being defined
  let depth = 0;      // open blocks (fnc / if / else)

  lines.forEach((line, i) => {
    const trimmed = line.trim();

    const fnc = FNC_RE.exec(line);
    if (fnc) {
      const params = splitArgs(fnc[3]).map((p) => {
        const m = PARAM_RE.exec(p);
        return m ? { name: m[2], type: m[3], kind: m[1] || 'param', default: m[4] } : { name: p, type: '?' };
      });
      current = {
        name: fnc[2],
        params,
        line: i,
        column: fnc[1].length,
        endLine: lines.length - 1,
        returns: fnc[4] || null,
        signature: `fnc ${fnc[2]}(${fnc[3].trim()})${fnc[4] ? ': ' + fnc[4] : ''}`,
      };
      functions.push(current);
      params.forEach((p) => variables.push({ ...p, line: i, column: line.indexOf(p.name, fnc[1].length + fnc[2].length), scope: current.name }));
      depth = 1;
      return;
    }

    const set = SET_RE.exec(line);
    if (set) {
      const kind = set[5] === '<==' ? 'borrow' : set[2] || 'owned';
      variables.push({ name: set[3], type: set[4], kind, line: i, column: set[1].length, scope: current && current.name });
    }

    const copy = COPY_RE.exec(line);
    if (copy) {
      const scope = current && current.name;
      const source = [...variables].reverse().find((v) => v.name === copy[1] && v.scope === scope);
      variables.push({ name: copy[2], type: source ? source.type : '?', kind: 'copy', line: i, column: line.lastIndexOf(copy[2]), scope });
    }

    // Track block depth to know when a function body ends.
    const closes = /^<-/.test(trimmed);
    const opens = /->$/.test(trimmed);
    if (closes) depth--;
    if (opens) depth++;
    if (current && depth <= 0) {
      current.endLine = i;
      current = null;
      depth = 0;
    }
  });

  function scopeAt(line) {
    const f = functions.find((fn) => line >= fn.line && line <= fn.endLine);
    return f ? f.name : null;
  }

  return { functions, variables, scopeAt };
}

/**
 * Build an index from `meng symbols` output (compiler data: exact block scopes
 * and ownership events). Lines and columns become 0-based like the regex index.
 */
function fromCompiler(data) {
  const functions = data.functions.map((f) => ({
    name: f.name,
    params: f.params,
    line: f.line - 1,
    column: f.column - 1,
    endLine: f.end - 1,
    signature: f.signature,
    returns: f.returns,
  }));
  const variables = data.variables.map((v) => ({
    name: v.name,
    type: v.type,
    kind: v.kind,
    line: v.line - 1,
    column: v.column - 1,
    endLine: v.end - 1,
    scope: v.function,
    // Moves (moved: true) and new values (moved: false) of this variable, in order.
    events: data.events
      .filter((e) => e.name === v.name && e.line >= v.line && e.line <= v.end)
      .map((e) => ({ line: e.line - 1, moved: e.moved })),
  }));
  function scopeAt(line) {
    const f = functions.find((fn) => line >= fn.line && line <= fn.endLine);
    return f ? f.name : null;
  }
  return { functions, variables, scopeAt, precise: true };
}

/** Line where the variable was moved, if it is still moved at `line` (compiler data only). */
function movedAt(v, line) {
  let moved = null;
  for (const e of v.events || []) {
    if (e.line >= line) break;
    moved = e.moved ? e.line : null;
  }
  return moved;
}

/**
 * Variables visible at `line`: same function, declared on or before that line,
 * and (with compiler data) inside their block. Each gets `movedAt` (0-based line
 * or null). Moved variables are left out unless `includeMoved` is set.
 */
function visibleVariables(index, line, includeMoved = false) {
  const scope = index.scopeAt(line);
  const seen = new Map();
  for (const v of index.variables) {
    const inBlock = v.endLine === undefined || line <= v.endLine;
    if (v.scope === scope && v.line <= line && inBlock) seen.set(v.name, { ...v, movedAt: movedAt(v, line) });
  }
  return [...seen.values()].filter((v) => includeMoved || v.movedAt === null);
}

/**
 * For signature help: the function being called at the end of `prefix`
 * and the index of the argument under the cursor.
 */
function callContext(prefix) {
  let depth = 0, inStr = false, commas = 0;
  for (let i = prefix.length - 1; i >= 0; i--) {
    const c = prefix[i];
    // Scanning backwards: treat quotes as toggles (escapes are rare in practice).
    if (c === '"' && prefix[i - 1] !== '\\') { inStr = !inStr; continue; }
    if (inStr) continue;
    if (c === ')') depth++;
    else if (c === '(') {
      if (depth === 0) {
        const m = new RegExp(`(${IDENT})\\s*$`).exec(prefix.slice(0, i));
        return m ? { name: m[1], argIndex: commas } : null;
      }
      depth--;
    } else if (c === ',' && depth === 0) commas++;
  }
  return null;
}

module.exports = {
  KEYWORDS, TYPES, CONSTANTS, BUILTINS,
  stripComments, splitArgs, indexSymbols, fromCompiler, visibleVariables, callContext, describeVar,
};
