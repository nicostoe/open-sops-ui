// Syntax colouring for the text editor: YAML, JSON and INI (also TOML and
// .conf files). Line based and forgiving: half-typed text must never break
// it. The result has exactly the characters of the input, so it lines up
// with the textarea it sits behind.
window.highlight = (() => {
  const esc = (s) => s.replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" }[c]));
  const span = (cls, s) => (!s ? "" : cls ? `<span class="t-${cls}">${esc(s)}</span>` : esc(s));

  // Splits a trailing comment off a line: `#` (or `;`) outside quotes, at the
  // start or after whitespace.
  function splitComment(line, marks) {
    let quote = null;
    for (let i = 0; i < line.length; i++) {
      const c = line[i];
      if (quote) {
        if (c === "\\" && quote === '"') i++;
        else if (c === quote) quote = null;
      } else if (c === '"' || c === "'") {
        quote = c;
      } else if (marks.includes(c) && (i === 0 || /\s/.test(line[i - 1]))) {
        return [line.slice(0, i), line.slice(i)];
      }
    }
    return [line, ""];
  }

  // A value: quoted string, number, boolean or null, or plain text.
  function scalar(text) {
    const [, lead, core, trail] = text.match(/^(\s*)(.*?)(\s*)$/s);
    let cls = "str";
    if (/^[-+]?(\d[\d_]*(\.\d*)?([eE][-+]?\d+)?|0x[0-9a-fA-F]+|0o[0-7]+|\.inf|\.nan)$/i.test(core)) cls = "num";
    else if (/^(true|false|yes|no|on|off|null|~)$/i.test(core)) cls = "bool";
    else if (/^[|>][-+]?\d*$/.test(core) || /^[&*][^\s]+$/.test(core)) cls = "punct";
    return esc(lead) + span(cls, core) + esc(trail);
  }

  function yamlLine(line) {
    if (/^\s*#/.test(line)) return span("comment", line);
    if (/^(---|\.\.\.)(\s.*)?$/.test(line)) return span("punct", line);
    const [body, comment] = splitComment(line, "#");
    let rest = body, out = "";
    const lead = rest.match(/^(\s*)((?:-(?:\s+|$))*)/);
    out += esc(lead[1]) + span("punct", lead[2]);
    rest = rest.slice(lead[0].length);
    const key = rest.match(/^("(?:[^"\\]|\\.)*"|'(?:[^']|'')*'|[^\s"'#\-[{][^:#]*?|-[^\s:#][^:#]*?)(\s*:)(?=\s|$)/);
    if (key) {
      out += span("key", key[1]) + span("punct", key[2]);
      rest = rest.slice(key[0].length);
    }
    return out + scalar(rest) + span("comment", comment);
  }

  function iniLine(line) {
    if (/^\s*[;#]/.test(line)) return span("comment", line);
    const section = line.match(/^(\s*)(\[\[?[^\]]*\]\]?)(.*)$/);
    if (section) return esc(section[1]) + span("section", section[2]) + span("comment", section[3]);
    const pair = line.match(/^(\s*)([^=:\s][^=]*?)(\s*=\s*)(.*)$/);
    if (!pair) return esc(line);
    const [value, comment] = splitComment(pair[4], "#;");
    return esc(pair[1]) + span("key", pair[2]) + span("punct", pair[3]) + scalar(value) + span("comment", comment);
  }

  function json(text) {
    const token = /("(?:[^"\\\n]|\\.)*"?)(\s*:)?|(-?\d+(?:\.\d+)?(?:[eE][-+]?\d+)?)|\b(true|false|null)\b|([{}[\],:])/g;
    let out = "", last = 0, m;
    while ((m = token.exec(text))) {
      out += esc(text.slice(last, m.index));
      if (m[1] !== undefined) out += m[2] !== undefined ? span("key", m[1]) + span("punct", m[2]) : span("str", m[1]);
      else if (m[3] !== undefined) out += span("num", m[3]);
      else if (m[4] !== undefined) out += span("bool", m[4]);
      else out += span("punct", m[5]);
      last = token.lastIndex;
    }
    return out + esc(text.slice(last));
  }

  const byLine = (fn) => (text) => text.split("\n").map(fn).join("\n");
  const languages = { yaml: byLine(yamlLine), ini: byLine(iniLine), json };

  // The language for a file: its sops format, or for a file encrypted as a
  // whole, its extension. Null when there is nothing to colour.
  function language(format, name) {
    if (languages[format]) return format;
    const ext = (name.match(/\.([a-z0-9]+)$/i) || [])[1]?.toLowerCase();
    return { yaml: "yaml", yml: "yaml", json: "json", ini: "ini", toml: "ini", conf: "ini", cfg: "ini", properties: "ini" }[ext] || null;
  }

  return { language, html: (lang, text) => languages[lang](text) };
})();
