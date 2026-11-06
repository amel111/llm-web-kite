"use strict";
(function () {
  var CONF = window.TEMPER || {};
  var convId = CONF.convId || null;
  var settings = CONF.settings || {};
  var currentModel = CONF.model || settings.model || "";

  var appEl = document.getElementById("app");
  var threadEl = document.getElementById("thread");
  var emptyEl = document.getElementById("emptyState");
  var ta = document.getElementById("composerInput");
  var sendBtn = document.getElementById("sendBtn");
  var sidebarList = document.getElementById("convList");

  var rows = [];
  var streamActive = false;
  var controller = null;

  // ---------- theme ----------
  var theme = localStorage.getItem("temper-theme") || "light";
  document.documentElement.setAttribute("data-theme", theme);

  function paintThemeBtn() {
    var b = document.getElementById("themeBtn");
    if (!b) return;
    var sun = b.querySelector(".icon-sun");
    var moon = b.querySelector(".icon-moon");
    if (sun) sun.style.display = theme === "dark" ? "none" : "";
    if (moon) moon.style.display = theme === "dark" ? "" : "none";
  }
  paintThemeBtn();

  function toggleTheme() {
    theme = theme === "dark" ? "light" : "dark";
    document.documentElement.setAttribute("data-theme", theme);
    localStorage.setItem("temper-theme", theme);
    paintThemeBtn();
  }
  var themeBtn = document.getElementById("themeBtn");
  if (themeBtn) themeBtn.addEventListener("click", toggleTheme);

  // ---------- sidebar ----------
  var hamburger = document.getElementById("hamburger");
  var collapsed = localStorage.getItem("temper-collapsed") === "1";
  if (collapsed) appEl.classList.add("collapsed");

  function toggleSidebar() {
    if (window.matchMedia("(max-width: 767px)").matches) {
      appEl.classList.toggle("nav-open");
    } else {
      collapsed = !collapsed;
      appEl.classList.toggle("collapsed", collapsed);
      localStorage.setItem("temper-collapsed", collapsed ? "1" : "0");
    }
  }
  if (hamburger) hamburger.addEventListener("click", toggleSidebar);
  var scrim = document.getElementById("scrim");
  if (scrim) scrim.addEventListener("click", function () { appEl.classList.remove("nav-open"); });

  var newChatBtn = document.getElementById("newChatBtn");
  if (newChatBtn) newChatBtn.addEventListener("click", function () {
    location.href = "/chat/new";
  });

  // conversations: search + menus
  var searchInput = document.getElementById("convSearch");
  if (searchInput) searchInput.addEventListener("input", function () {
    var q = searchInput.value.toLowerCase();
    var items = sidebarList.querySelectorAll(".conv-item");
    for (var i = 0; i < items.length; i++) {
      var t = items[i].querySelector(".conv-title");
      items[i].style.display = !q || (t && t.textContent.toLowerCase().indexOf(q) !== -1) ? "" : "none";
    }
  });

  function closeMenus() {
    var menus = sidebarList.querySelectorAll(".conv-menu");
    for (var i = 0; i < menus.length; i++) menus[i].style.display = "none";
  }
  document.addEventListener("click", function () { closeMenus(); });

  if (sidebarList) sidebarList.addEventListener("click", function (e) {
    var item = e.target.closest(".conv-item");
    if (!item) return;
    var menuBtn = e.target.closest(".conv-menu-btn");
    var menu = item.querySelector(".conv-menu");
    if (menuBtn) {
      e.preventDefault();
      e.stopPropagation();
      var open = menu && menu.style.display === "block";
      closeMenus();
      if (menu && !open) menu.style.display = "block";
      return;
    }
    if (e.target.closest(".conv-menu")) {
      e.preventDefault();
      e.stopPropagation();
      if (e.target.closest(".act-rename")) startRename(item);
      if (e.target.closest(".act-delete")) deleteConv(+item.dataset.id, item);
      return;
    }
  });

  function startRename(item) {
    var id = +item.dataset.id;
    var titleEl = item.querySelector(".conv-title");
    if (!titleEl || item.querySelector(".conv-rename")) return;
    var old = titleEl.textContent;
    var input = document.createElement("input");
    input.className = "conv-rename";
    input.value = old;
    item.insertBefore(input, titleEl);
    titleEl.style.display = "none";
    input.focus();
    input.select();
    var done = false;
    function save() {
      if (done) return;
      done = true;
      var val = input.value.trim() || old;
      fetch("/api/conversations/" + id + "/rename", {
        method: "POST", headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ title: val })
      }).then(function () {
        titleEl.textContent = val;
        cleanup();
      }).catch(function () { cleanup(); });
    }
    function cleanup() {
      if (input.parentNode) item.removeChild(input);
      titleEl.style.display = "";
    }
    input.addEventListener("keydown", function (e) {
      if (e.key === "Enter") { e.preventDefault(); save(); }
      else if (e.key === "Escape") { done = true; cleanup(); }
    });
    input.addEventListener("blur", save);
    var inner = item.querySelector(".conv-menu");
    if (inner) inner.style.display = "none";
  }

  var pendingDelete = null;
  function deleteConv(id, item) {
    var title = item.querySelector(".conv-title");
    pendingDelete = { id: id, item: item, title: title ? title.textContent : "this conversation" };
    var titleEl = document.getElementById("deleteConvTitle");
    if (titleEl) titleEl.textContent = pendingDelete.title;
    var deleteModal = document.getElementById("deleteModal");
    if (deleteModal) deleteModal.classList.add("open");
  }

  function closeDeleteModal() {
    var deleteModal = document.getElementById("deleteModal");
    if (deleteModal) deleteModal.classList.remove("open");
    pendingDelete = null;
  }

  function confirmDelete() {
    if (!pendingDelete) return;
    var target = pendingDelete;
    var confirmBtn = document.getElementById("deleteConfirm");
    if (confirmBtn) confirmBtn.disabled = true;
    fetch("/api/conversations/" + target.id, { method: "DELETE" }).then(function (response) {
      if (!response.ok) throw new Error("HTTP " + response.status);
      if (target.id === convId) location.href = "/";
      else target.item.remove();
      closeDeleteModal();
    }).catch(function (error) {
      snack("Delete failed: " + error.message);
    }).finally(function () {
      if (confirmBtn) confirmBtn.disabled = false;
    });
  }

  function insertConvItem(id, title) {
    var sec = sidebarList.querySelector('[data-group="Today"]');
    if (!sec) {
      sec = document.createElement("div");
      var label = document.createElement("div");
      label.className = "conv-group";
      label.textContent = "Today";
      sidebarList.appendChild(label);
      sec = document.createElement("div");
      sec.setAttribute("data-group", "Today");
      sidebarList.appendChild(sec);
    }
    var a = document.createElement("a");
    a.className = "conv-item active";
    a.href = "/chat/" + id;
    a.dataset.id = id;
    a.innerHTML = '<span class="conv-title"></span>' +
      '<button class="conv-menu-btn" type="button" aria-label="Menu">' +
      '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="5" cy="12" r="1.5"/><circle cx="12" cy="12" r="1.5"/><circle cx="19" cy="12" r="1.5"/></svg>' +
      '</button>' +
      '<div class="conv-menu" style="display:none;">' +
      '<button type="button" class="act-rename">Rename</button>' +
      '<button type="button" class="act-delete danger">Delete</button>' +
      '</div>';
    a.querySelector(".conv-title").textContent = title;
    var others = sec.querySelectorAll(".conv-item");
    for (var i = 0; i < others.length; i++) others[i].classList.remove("active");
    sec.insertBefore(a, sec.firstChild);
    sidebarList.scrollTop = 0;
  }

  // ---------- model picker ----------
  var UNLOAD_ICON = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18.36 6.64a9 9 0 1 1-12.73 0"/><path d="M12 2v10"/></svg>';
  function loadModel(name) {
    return fetch("/api/model_load", {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ model: name })
    });
  }
  function fillDropdown(dd) {
    return Promise.all([
      fetch("/api/models").then(function (r) { return r.json(); }),
      fetch("/api/model_status").then(function (r) { return r.json(); })
    ]).then(function (results) {
      var d = results[0];
      var status = results[1];
      var running = status.running || [];
      function isLoaded(name) {
        var base = name.split(":")[0];
        for (var n = 0; n < running.length; n++) {
          if (running[n] === name || running[n].split(":")[0] === base) return true;
        }
        return false;
      }
      dd.innerHTML = "";
      var list = (d.data || []).map(function (m) { return m.id; });
      if (!list.length) list = [currentModel];
      for (var i = 0; i < list.length; i++) {
        var row = document.createElement("div");
        row.className = "model-row";
        var b = document.createElement("button");
        b.type = "button";
        b.className = "model-name";
        b.textContent = list[i];
        if (list[i] === currentModel) b.classList.add("selected");
        b.addEventListener("click", function (name) {
          return function () {
            currentModel = name;
            fetch("/api/settings", {
              method: "POST", headers: { "Content-Type": "application/json" },
              body: JSON.stringify({ model: name })
            }).then(function () {
              var lbl = document.getElementById("modelName");
              if (lbl) lbl.textContent = name;
              dd.style.display = "none";
              snack("Loading (" + name.replace(":latest", "") + ")…");
              loadModel(name).then(function (r) { return r.json(); }).then(function (res) {
                snack(res.ok ? "Model: " + name : "Load failed");
                fillDropdown(dd);
              }).catch(function () { snack("Load failed"); });
              pollModelStatus();
            });
          };
        }(list[i]));
        var ub = document.createElement("button");
        ub.type = "button";
        ub.className = "model-unload";
        ub.title = "Unload model";
        ub.disabled = !isLoaded(list[i]);
        ub.setAttribute("aria-label", ub.disabled ? "Model is not loaded" : "Unload model");
        ub.innerHTML = UNLOAD_ICON;
        ub.addEventListener("click", function (name) {
          return function () {
            if (this.disabled) return;
            snack("Unloading " + name + "…");
            fetch("/api/model_unload", {
              method: "POST", headers: { "Content-Type": "application/json" },
              body: JSON.stringify({ model: name })
            }).then(function (r) { return r.json(); }).then(function (res) {
              if (res.ok) snack("Model unloaded");
              else snack("Unload failed");
              fillDropdown(dd);
              pollModelStatus();
            }).catch(function () { snack("Unload failed"); });
          };
        }(list[i]));
        row.appendChild(b);
        row.appendChild(ub);
        dd.appendChild(row);
      }
    }).catch(function () {});
  }
  var modelBtn = document.getElementById("modelBtn");
  var modelDropdown = document.getElementById("modelDropdown");
  if (modelBtn && modelDropdown) {
    modelBtn.addEventListener("click", function (e) {
      e.stopPropagation();
      if (modelDropdown.style.display === "block") { modelDropdown.style.display = "none"; return; }
      modelDropdown.style.display = "block";
      fillDropdown(modelDropdown);
    });
    document.addEventListener("click", function () { modelDropdown.style.display = "none"; });
    modelDropdown.addEventListener("click", function (e) { e.stopPropagation(); });
  }

  // ---------- model load indicator ----------
  var modelLed = document.getElementById("modelLed");
  function pollModelStatus() {
    fetch("/api/model_status").then(function (r) { return r.json(); }).then(function (d) {
      if (!modelLed) return;
      if (!!d.loaded !== modelLed.classList.contains("loaded")) {
        modelLed.classList.remove("loaded");
        if (d.loaded) { void modelLed.offsetWidth; modelLed.classList.add("loaded"); }
      }
      modelLed.title = d.loaded ? (d.model + " loaded") : (d.model + " not loaded");
    }).catch(function () {});
  }
  if (modelLed) {
    pollModelStatus();
    setInterval(pollModelStatus, 2000);
  }

  // ---------- markdown pipeline ----------
  var rafPending = false;
  var rafHandle = null;

  function md(text) {
    var pre = wrapBareLatex(text);
    var ex = extractMath(pre);
    var html = marked.parse(ex.text);
    return DOMPurify.sanitize(renderMathBlocks(html, ex.blocks));
  }

  // Extract math spans BEFORE marked.parse: marked treats \( as escape and
  // would eat the delimiters. Replace each with a null-char placeholder, then
  // substitute KaTeX HTML after markdown rendering.
  function extractMath(text) {
    var blocks = [];
    function ph(latex, display) {
      var n = blocks.length;
      blocks.push({ latex: latex, display: display });
      return "\u0000M" + n + "\u0000";
    }
    var out = "", i = 0, inFence = false;
    while (i < text.length) {
      if (text.substr(i, 3) === "```") { inFence = !inFence; out += "```"; i += 3; continue; }
      if (inFence) { out += text[i]; i++; continue; }
      if (text.substr(i, 2) === "$$") {
        var e = text.indexOf("$$", i + 2);
        if (e !== -1) { out += ph(text.slice(i + 2, e), true); i = e + 2; continue; }
      }
      if (text.substr(i, 2) === "\\[") {
        var e2 = text.indexOf("\\]", i + 2);
        if (e2 !== -1) { out += ph(text.slice(i + 2, e2), true); i = e2 + 2; continue; }
      }
      if (text.substr(i, 2) === "\\(") {
        var e3 = text.indexOf("\\)", i + 2);
        if (e3 !== -1) { out += ph(text.slice(i + 2, e3), false); i = e3 + 2; continue; }
      }
      if (text[i] === "$" && (i === 0 || text[i - 1] !== "\\")) {
        var e4 = text.indexOf("$", i + 1);
        var inner = e4 === -1 ? null : text.slice(i + 1, e4);
        if (inner && inner.length <= 300 && inner.indexOf("\n") === -1) {
          out += ph(inner, false); i = e4 + 1; continue;
        }
      }
      out += text[i]; i++;
    }
    return { text: out, blocks: blocks };
  }

  function renderMathBlocks(html, blocks) {
    if (!blocks || !blocks.length || !window.katex) return html;
    return html.replace(/\u0000M(\d+)\u0000/g, function (_, n) {
      var b = blocks[+n];
      if (!b) return "";
      try {
        return window.katex.renderToString(b.latex, { displayMode: b.display, throwOnError: false });
      } catch (e) { return b.latex; }
    });
  }

  // ---------- math pre-pass: wrap bare LaTeX in $...$ ----------
  // Small models often emit LaTeX without any $ / \( \) delimiters,
  // e.g. "(\frac{4}{3}x^3 + C)". Detect parenthesized groups that look
  // like math and wrap them so KaTeX auto-render picks them up.
  var LATEX_CMD = /\\(?:frac|sqrt|cdot|times|pm|mp|div|sum|prod|int|oint|lim|alpha|beta|gamma|delta|epsilon|varepsilon|zeta|eta|theta|iota|kappa|lambda|mu|nu|xi|rho|sigma|tau|upsilon|phi|chi|psi|omega|Gamma|Delta|Theta|Lambda|Sigma|Phi|Psi|Omega|partial|nabla|infty|le|leq|ge|geq|ne|neq|approx|equiv|sim|propto|rightarrow|Rightarrow|leftarrow|Leftarrow|to|mapsto|in|notin|ni|subset|subseteq|supset|forall|exists|mathbb|mathrm|mathcal|mathsf|text|textrm|quad|qquad|begin|end|binom|left|right|big|Big|bigg|Bigg|atop|choose|over)\b/;

  function wrapBareLatex(text) {
    if (/\$|\\\(|\\\[/.test(text)) return text; // already delimited
    if (!LATEX_CMD.test(text)) return text;     // no LaTeX anywhere
    var out = "", i = 0;
    while (i < text.length) {
      var open = text.indexOf("(", i);
      if (open === -1) { out += text.slice(i); break; }
      var depth = 0, j = open;
      for (; j < text.length; j++) {
        if (text[j] === "(") depth++;
        else if (text[j] === ")") { depth--; if (depth === 0) break; }
      }
      if (j >= text.length) { out += text.slice(i); break; } // unbalanced, leave rest
      var inner = text.slice(open + 1, j);
      // strip LaTeX commands, then reject groups that still contain long
      // plain words (prose, not math)
      var stripped = inner.replace(/\\[a-zA-Z]+/g, "");
      if (LATEX_CMD.test(inner) && !/[A-Za-z]{5,}/.test(stripped)) {
        out += text.slice(i, open) + "($" + inner + "$)";
      } else {
        out += text.slice(i, j + 1);
      }
      i = j + 1;
    }
    return out;
  }

  // ---------- math: render $...$ / $$...$$ via KaTeX ----------
  function renderMath(el) {
    if (!window.renderMathInElement || !el) return;
    try {
      window.renderMathInElement(el, {
        delimiters: [
          { left: "$$", right: "$$", display: true },
          { left: "$", right: "$", display: false },
          { left: "\\(", right: "\\)", display: false },
          { left: "\\[", right: "\\]", display: true }
        ],
        throwOnError: false,
        ignoredTags: ["script", "noscript", "style", "textarea", "pre", "code"],
        ignoredClasses: ["pending-code"]
      });
    } catch (e) {}
  }

  function splitOpenFence(text) {
    var idx = text.lastIndexOf("```");
    if (idx === -1) return { head: text, tail: null };
    var before = text.slice(0, idx);
    var fences = (before.match(/```/g) || []).length;
    if (fences % 2 === 1) return { head: text, tail: null };
    return { head: before, tail: text.slice(idx) };
  }

  function renderInto(el, text) {
    var parts = splitOpenFence(text);
    var html = md(parts.head);
    if (parts.tail !== null) html += '<pre class="pending-code"><code></code></pre>';
    el.innerHTML = html;
    if (parts.tail !== null) {
      var code = el.querySelector(".pending-code code");
      if (code) code.textContent = parts.tail;
    }
    renderMath(el);
  }

  function finalize(el, text) {
    if (rafPending && rafHandle !== null) cancelAnimationFrame(rafHandle);
    rafPending = false;
    el.innerHTML = md(text || "");
    var blocks = el.querySelectorAll("pre code");
    for (var i = 0; i < blocks.length; i++) {
      try { hljs.highlightElement(blocks[i]); } catch (e) {}
    }
    renderMath(el);
    addCopyButtons(el);
  }

  function scheduleRender(el, text) {
    if (rafPending) return;
    rafPending = true;
    rafHandle = requestAnimationFrame(function () {
      rafPending = false;
      renderInto(el, text);
      if (followStream) forceScrollBottom();
    });
  }

  // ---------- copy (message + code blocks) ----------
  function copyText(text) {
    if (navigator.clipboard && navigator.clipboard.writeText) {
      return navigator.clipboard.writeText(text).catch(function () { return legacyCopy(text); });
    }
    return legacyCopy(text);
  }
  function legacyCopy(text) {
    return new Promise(function (resolve, reject) {
      try {
        var t = document.createElement("textarea");
        t.value = text;
        t.style.position = "fixed";
        t.style.opacity = "0";
        document.body.appendChild(t);
        t.select();
        var ok = document.execCommand("copy");
        document.body.removeChild(t);
        if (ok) resolve(); else reject(new Error("copy failed"));
      } catch (e) { reject(e); }
    });
  }

  function addCopyButtons(el) {
    var pres = el.querySelectorAll("pre");
    for (var i = 0; i < pres.length; i++) {
      var pre = pres[i];
      if (pre.querySelector(".copy-btn")) continue;
      var btn = document.createElement("button");
      btn.type = "button";
      btn.className = "copy-btn";
      btn.setAttribute("aria-label", "Copy code");
      btn.title = "Copy";
      icon(btn, '<rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>', 12);
      btn.addEventListener("click", (function (code) {
        return function () {
          copyText(code.textContent).then(
            function () { snack("Copied"); },
            function () { snack("Copy failed"); }
          );
        };
      })(pre.querySelector("code")));
      pre.appendChild(btn);
    }
  }

  function icon(parent, paths, size) {
    var s = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    s.setAttribute("viewBox", "0 0 24 24");
    s.setAttribute("fill", "none");
    s.setAttribute("stroke", "currentColor");
    s.setAttribute("stroke-width", "2");
    s.setAttribute("stroke-linecap", "round");
    s.setAttribute("width", (size || 16) + "px");
    s.setAttribute("height", (size || 16) + "px");
    s.innerHTML = paths;
    parent.appendChild(s);
    return s;
  }

  var ICONS = {
    copyMsg: '<rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>',
    regen: '<path d="M21 12a9 9 0 1 1-3-6.7"/><path d="M21 3v6h-6"/>',
    edit: '<path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z"/>',
    prev: '<path d="m15 18-6-6 6-6"/>',
    next: '<path d="m9 18 6-6-6-6"/>'
  };

  // ---------- message rows ----------
  function appendRow(role) {
    var row = document.createElement("div");
    row.className = "row msg-" + role;
    threadEl.appendChild(row);
    return row;
  }

  function buildUserRow(m) {
    var row = appendRow("user");
    var wrap = document.createElement("div");
    wrap.className = "bubble-wrap";
    var bubble = document.createElement("div");
    bubble.className = "bubble";
    bubble.textContent = m.content;
    wrap.appendChild(bubble);
    var acts = document.createElement("div");
    acts.className = "msg-actions right";
    var eb = document.createElement("button");
    eb.type = "button";
    eb.className = "icon-btn";
    eb.title = "Edit";
    icon(eb, ICONS.edit, 14);
    eb.addEventListener("click", function () { startEdit(row); });
    acts.appendChild(eb);
    wrap.appendChild(acts);
    row.appendChild(wrap);
    rows.push({ el: row, role: "user", id: m.id, content: m.content,
      branchOf: m.branch_group || m.branch_of || m.id, versions: m.versions || [m], versionIndex: (m.versions || [m]).length - 1 });
    return row;
  }

  function buildAssistantRow(m) {
    var row = appendRow("assistant");
    finalize(row, m.content || "");
    appendAssistantActions(row, m.content || "", m);
    rows.push({ el: row, role: "assistant", id: m.id, content: m.content, branchOf: null, pairId: null });
    return row;
  }

  function appendAssistantActions(row, content, m) {
    var old = row.querySelector(".msg-actions");
    if (old) old.remove();
    var acts = document.createElement("div");
    acts.className = "msg-actions";
    var cb = document.createElement("button");
    cb.type = "button"; cb.className = "icon-btn"; cb.title = "Copy";
    icon(cb, ICONS.copyMsg, 14);
    cb.addEventListener("click", function () {
      copyText(content).then(function () { snack("Copied"); }, function () { snack("Copy failed"); });
    });
    var rb = document.createElement("button");
    rb.type = "button"; rb.className = "icon-btn"; rb.title = "Regenerate";
    icon(rb, ICONS.regen, 14);
    rb.addEventListener("click", function () { regenerate(); });
    acts.appendChild(cb);
    acts.appendChild(rb);
    if (m && (m.model || (m.completion_tokens && m.elapsed_s))) {
      var meta = document.createElement("span");
      meta.className = "meta";
      var parts = [];
      if (m.model) parts.push(m.model.replace(/:latest$/, ""));
      if (m.completion_tokens && m.elapsed_s) parts.push((m.completion_tokens / m.elapsed_s).toFixed(1) + " tok/s");
      meta.textContent = parts.join(" · ");
      acts.appendChild(meta);
    }
    row.appendChild(acts);
  }

  function renderHistory() {
    var oldRows = threadEl.querySelectorAll(".row");
    for (var i = 0; i < oldRows.length; i++) oldRows[i].remove();
    rows = [];
    var history = CONF.messages || [];
    if (!history.length) {
      if (emptyEl) emptyEl.style.display = "flex";
      return;
    }
    if (emptyEl) emptyEl.style.display = "none";
    var latestByBranch = {};
    for (var i = 0; i < history.length; i++) {
      if (history[i].role !== "user") continue;
      var branch = history[i].branch_group || history[i].branch_of || history[i].id;
      latestByBranch[branch] = history[i].id;
    }
    var staleAssistantIds = {};
    for (var k = 0; k + 1 < history.length; k++) {
      if (history[k].role !== "user" || history[k + 1].role !== "assistant") continue;
      var staleKey = history[k].branch_group || history[k].branch_of || history[k].id;
      if (latestByBranch[staleKey] !== history[k].id) staleAssistantIds[history[k + 1].id] = true;
    }
    for (var j = 0; j < history.length; j++) {
      var message = history[j];
      if (message.role === "user") {
        var key = message.branch_group || message.branch_of || message.id;
        if (latestByBranch[key] !== message.id) continue;
        var versions = history.filter(function (candidate) {
          return candidate.role === "user" &&
            (candidate.branch_group || candidate.branch_of || candidate.id) === key;
        });
        buildUserRow(Object.assign({}, message, { versions: versions }));
        if (versions.length > 1 && j + 1 < history.length && history[j + 1].role === "assistant") {
          var userEntry = rows[rows.length - 1];
          var assistantRow = buildAssistantRow(history[j + 1]);
          makePager(userEntry, assistantRow);
          j++;
        }
      } else {
        if (staleAssistantIds[message.id]) continue;
        buildAssistantRow(message);
      }
    }
    threadEl.scrollTop = threadEl.scrollHeight;
  }

  // ---------- edit / resend user message (versions) ----------
  function setBubbleText(row, text) {
    var bubble = row.querySelector(".bubble");
    if (bubble) bubble.textContent = text;
  }
  function rowIndex(rowEl) {
    for (var i = 0; i < rows.length; i++) if (rows[i].el === rowEl) return i;
    return -1;
  }
  function makePager(userEntry, assistantRow) {
    var versions = userEntry.versions || [];
    var userRow = userEntry.el;
    if (assistantRow.querySelector(".version-pager")) return;
    if (versions.length <= 1) return;
    var pager = document.createElement("div");
    pager.className = "version-pager";
    var prevBtn = document.createElement("button"); prevBtn.type = "button"; prevBtn.className = "icon-btn"; prevBtn.title = "Previous version";
    icon(prevBtn, ICONS.prev, 14);
    var nextBtn = document.createElement("button"); nextBtn.type = "button"; nextBtn.className = "icon-btn"; nextBtn.title = "Next version";
    icon(nextBtn, ICONS.next, 14);
    var lbl = document.createElement("span"); lbl.className = "version-count";
    pager.appendChild(prevBtn); pager.appendChild(lbl); pager.appendChild(nextBtn);
    assistantRow.appendChild(pager);
    var sel = userEntry.versionIndex == null ? versions.length - 1 : userEntry.versionIndex;
    setBubbleText(userRow, versions[sel].content);
    lbl.textContent = (sel + 1) + "/" + versions.length;
    function updateDisabled() {
      prevBtn.disabled = sel <= 0; nextBtn.disabled = sel >= versions.length - 1;
    }
    function go(n) {
      sel = Math.max(0, Math.min(versions.length - 1, n));
      setBubbleText(userRow, versions[sel].content);
      lbl.textContent = (sel + 1) + "/" + versions.length;
      updateDisabled();
    }
    updateDisabled();
    prevBtn.addEventListener("click", function () { go(sel - 1); });
    nextBtn.addEventListener("click", function () { go(sel + 1); });
  }
  function startEdit(row) {
    if (streamActive) return;
    var entry = null;
    for (var i = 0; i < rows.length; i++) {
      if (rows[i].el === row && rows[i].role === "user") {
        entry = rows[i];
        break;
      }
    }
    if (!entry) return;
    var wrap = row.querySelector(".bubble-wrap");
    var bubble = row.querySelector(".bubble");
    var input = document.createElement("textarea");
    input.className = "user-edit-input";
    input.value = bubble ? bubble.textContent : entry.content;
    wrap.replaceChild(input, bubble);
    input.focus();
    var editing = true;
    function cancelEdit() {
      if (!editing) return;
      editing = false;
      if (input.parentNode === wrap) wrap.replaceChild(bubble, input);
    }
    input.addEventListener("keydown", function (e) {
      if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); submit(); }
      else if (e.key === "Escape") cancelEdit();
    });
    input.addEventListener("blur", function () {
      cancelEdit();
    });
    function submit() {
      var val = input.value.trim();
      if (!val) { cancelEdit(); return; }
      editing = false;
      input.disabled = true;
      doEdit(entry, val, row);
    }
  }

  function doEdit(entry, content, userRow) {
    if (streamActive) return;
    // truncate DOM rows after the edited entry (keep the entry itself)
    var idx = rowIndex(userRow);
    var after = rows.slice(idx + 1);
    for (var j = 0; j < after.length; j++) {
      after[j].el.remove();
    }
    rows.length = idx + 1;
    setBubbleText(userRow, content);
    entry.content = content;
    entry.versions = (entry.versions || []).concat([{ id: Date.now(), role: "user", content: content }]);
    entry.versionIndex = entry.versions.length - 1;
    streamReply({ message_id: entry.id, conversation_id: convId, content: content }, "/api/chat/edit");
  }

  // resend the active version (no edit)
  function launchResend(entry, userRow, content) {
    if (streamActive) return;
    var idx = rowIndex(userRow);
    var after = rows.slice(idx + 1);
    for (var j = 0; j < after.length; j++) after[j].el.remove();
    rows.length = idx + 1;
    setBubbleText(userRow, content);
    entry.versions = (entry.versions || []).concat([{ id: Date.now(), role: "user", content: content }]);
    entry.versionIndex = entry.versions.length - 1;
    streamReply({ message_id: entry.id, conversation_id: convId, content: content }, "/api/chat/edit");
  }

  function findEntry(id) {
    for (var i = 0; i < rows.length; i++) if (rows[i].id === id) return rows[i];
    return null;
  }

  // ---------- regenerate ----------
  function regenerate() {
    if (streamActive || !convId) return;
    var removed = false;
    for (var i = rows.length - 1; i >= 0; i--) {
      if (rows[i].role === "assistant") {
        rows[i].el.remove();
        rows.splice(i, 1);
        removed = true;
        break;
      }
    }
    if (!removed) return;
    streamReply({ conversation_id: convId }, "/api/chat/regenerate");
  }

  // ---------- streaming ----------
  function streamReply(body, url) {
    emptyEl.style.display = "none";
    var row = appendRow("assistant");
    row.classList.add("stream-caret");
    rows.push({ el: row, role: "assistant", id: null, content: "", branchOf: null, pairId: null });
    streamActive = true;
    setSendState();
    controller = new AbortController();
    var full = "";
    var lastUsage = null;
    var loadingToast = false;
    var toastFinished = false;
    var loadingToastTimer = setTimeout(function () {
      if (toastFinished) return;
      loadingToast = true;
      snack("Loading model (" + currentModel.replace(":latest", "") + ")…");
    }, 350);
    forceScrollBottom();

    fetch("/api/model_status").then(function (r) { return r.json(); }).then(function (status) {
      if (toastFinished) return;
      clearTimeout(loadingToastTimer);
      if (status && !status.loaded) {
        loadingToast = true;
        snack("Loading model (" + (status.model || currentModel).replace(":latest", "") + ")…");
      }
    }).catch(function () {
      if (!toastFinished) {
        loadingToast = true;
        snack("Loading model (" + currentModel.replace(":latest", "") + ")…");
      }
    });

    fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
      signal: controller.signal
    }).then(function (r) {
      if (!r.ok) throw new Error("HTTP " + r.status);
      return readStream(r.body.getReader(), row);
    }).then(function () {
      toastFinished = true;
      clearTimeout(loadingToastTimer);
      finishStream(row, full, lastUsage);
      if (loadingToast) snack("Model ready");
    }).catch(function (e) {
      toastFinished = true;
      clearTimeout(loadingToastTimer);
      if (e && e.name === "AbortError") {
        finishStream(row, full, lastUsage);
        if (loadingToast) snack("Model loading stopped");
      } else {
        row.classList.remove("stream-caret");
        var err = document.createElement("div");
        err.className = "msg-error";
        err.textContent = "error: " + ((e && e.message) || String(e));
        row.appendChild(err);
        streamActive = false;
        setSendState();
        if (loadingToast) snack("Model load failed");
      }
    });

    function readStream(reader, rowEl) {
      var dec = new TextDecoder();
      var buf = "";
      function pump() {
        return reader.read().then(function (out) {
          if (out.done) return;
          buf += dec.decode(out.value, { stream: true });
          var lines = buf.split("\n");
          buf = lines.pop();
          for (var i = 0; i < lines.length; i++) {
            var line = lines[i];
            if (line.indexOf("data: ") !== 0) continue;
            var payload = line.slice(6).trim();
            if (payload === "[DONE]") continue;
            var j = null;
            try { j = JSON.parse(payload); } catch (e2) { continue; }
            if (j.error) throw new Error(j.error);
            var delta = j.choices && j.choices[0] && j.choices[0].delta && j.choices[0].delta.content;
            if (delta) { full += delta; scheduleRender(rowEl, full); }
            if (j.usage) lastUsage = j.usage;
          }
          return pump();
        });
      }
      return pump();
    }
  }

  function finishStream(row, full, lastUsage) {
    row.classList.remove("stream-caret");
    streamActive = false;
    setSendState();
    pollModelStatus();
    var entry = null;
    for (var i = 0; i < rows.length; i++) if (rows[i].el === row) entry = rows[i];
    if (entry) entry.content = full;
    if (full) {
      finalize(row, full);
      appendAssistantActions(row, full, lastUsage ? {
        completion_tokens: lastUsage.completion_tokens,
        elapsed_s: lastUsage.elapsed_s,
        model: lastUsage.model
      } : null);
      var assistantIndex = rows.indexOf(entry);
      if (assistantIndex > 0) {
        var userEntry = rows[assistantIndex - 1];
        if (userEntry && userEntry.role === "user") makePager(userEntry, row);
      }
    }
    stickToBottom();
  }

  // ---------- composer ----------
  function setSendState() {
    sendBtn.classList.toggle("stop", streamActive);
    sendBtn.disabled = false;
    ta.placeholder = streamActive ? "Generating…" : "Message…";
  }

  function autosize() {
    ta.style.height = "auto";
    ta.style.height = Math.min(ta.scrollHeight, 200) + "px";
  }
  ta.addEventListener("input", autosize);

  async function send() {
    var text = ta.value.trim();
    if (streamActive) { if (controller) controller.abort(); return; }
    if (!text) return;
    var createdConversation = false;
    if (!convId) {
      var r = await fetch("/api/conversations/new", { method: "POST" });
      var d = await r.json();
      convId = d.id;
      createdConversation = true;
      insertConvItem(convId, text.slice(0, 40));
    }
    emptyEl.style.display = "none";
    ta.value = "";
    autosize();
    buildUserRow({ role: "user", id: null, content: text });
    forceScrollBottom();
    streamReply({ conversation_id: convId, message: text, skip_add_user: createdConversation }, "/api/chat/send");
  }
  sendBtn.addEventListener("click", send);
  ta.addEventListener("keydown", function (e) {
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); send(); }
  });

  // suggestion chips
  document.querySelectorAll(".chip").forEach(function (chip) {
    chip.addEventListener("click", function () {
      ta.value = chip.dataset.text || chip.textContent;
      autosize();
      send();
    });
  });

  function stickToBottom() {
    if (threadEl.scrollHeight - threadEl.scrollTop - threadEl.clientHeight < 48) {
      threadEl.scrollTop = threadEl.scrollHeight;
    }
  }
  var followStream = false;
  function forceScrollBottom() {
    threadEl.scrollTop = threadEl.scrollHeight;
    followStream = true;
  }
  threadEl.addEventListener("scroll", function () {
    if (!streamActive) return;
    followStream = (threadEl.scrollHeight - threadEl.scrollTop - threadEl.clientHeight) < 24;
  });

  // ---------- settings modal ----------
  var modal = document.getElementById("settingsModal");
  var settingsBtn = document.getElementById("settingsBtn");
  var modalClose = document.getElementById("modalClose");
  var deleteModal = document.getElementById("deleteModal");
  var deleteCancel = document.getElementById("deleteCancel");
  var deleteConfirm = document.getElementById("deleteConfirm");
  var statsTimer = null;

  function openModal() {
    fillSettingsForm();
    fillModelSelect();
    refreshStats();
    statsTimer = setInterval(refreshStats, 5000);
    modal.classList.add("open");
  }
  function closeModal() {
    clearInterval(statsTimer);
    statsTimer = null;
    modal.classList.remove("open");
  }
  if (settingsBtn) settingsBtn.addEventListener("click", openModal);
  if (modalClose) modalClose.addEventListener("click", closeModal);
  if (modal) modal.addEventListener("click", function (e) { if (e.target === modal) closeModal(); });
  if (deleteCancel) deleteCancel.addEventListener("click", closeDeleteModal);
  if (deleteConfirm) deleteConfirm.addEventListener("click", confirmDelete);
  if (deleteModal) deleteModal.addEventListener("click", function (e) {
    if (e.target === deleteModal) closeDeleteModal();
  });
  document.addEventListener("keydown", function (e) {
    if (e.key !== "Escape") return;
    if (deleteModal && deleteModal.classList.contains("open")) closeDeleteModal();
    else closeModal();
  });

  function fillSettingsForm() {
    var set = function (id, v) { var el = document.getElementById(id); if (el != null && v != null) el.value = v; };
    set("s_ollama_url", settings.ollama_url);
    set("s_input_price", settings.input_price);
    set("s_output_price", settings.output_price);
    set("s_usd_to_idr", settings.usd_to_idr);
  }

  function fillModelSelect() {
    var sel = document.getElementById("s_model");
    if (!sel) return;
    fetch("/api/models").then(function (r) { return r.json(); }).then(function (d) {
      sel.innerHTML = "";
      var list = (d.data || []).map(function (m) { return m.id; });
      if (!list.length) list = [currentModel];
      for (var i = 0; i < list.length; i++) {
        var o = document.createElement("option");
        o.value = list[i];
        o.textContent = list[i];
        if (list[i] === currentModel) o.selected = true;
        sel.appendChild(o);
      }
    }).catch(function () {});
  }

  var fmtUsd = function (v) {
    if (!isFinite(v)) v = 0;
    var s = v.toFixed(6).replace(/(\.\d*?)0+$/, "$1").replace(/\.$/, "");
    return "$" + s;
  };
  var fmtIdr = function (v) {
    if (!isFinite(v)) v = 0;
    return "Rp " + Math.round(v).toLocaleString("id-ID");
  };

  function refreshStats() {
    fetch("/api/stats").then(function (r) { return r.json(); }).then(function (d) {
      var t = d.totals || {};
      var setText = function (id, v) { var el = document.getElementById(id); if (el) el.textContent = v; };
      setText("s_req", (t.requests || 0).toLocaleString("id-ID"));
      setText("s_in", (t.prompt_tokens || 0).toLocaleString("id-ID"));
      setText("s_out", (t.completion_tokens || 0).toLocaleString("id-ID"));
      setText("s_tot", (t.total_tokens || 0).toLocaleString("id-ID"));
      var ip = parseFloat(document.getElementById("s_input_price").value) || 0;
      var op = parseFloat(document.getElementById("s_output_price").value) || 0;
      var rate = parseFloat(document.getElementById("s_usd_to_idr").value) || 0;
      var usd = (t.prompt_tokens || 0) / 1000 * ip + (t.completion_tokens || 0) / 1000 * op;
      setText("s_usd", fmtUsd(usd));
      setText("s_idr", fmtIdr(usd * rate));
      var tb = document.getElementById("recentBody");
      if (tb) {
        tb.innerHTML = "";
        var rec = (d.recent || []).slice(-20).reverse();
        for (var i = 0; i < rec.length; i++) {
          var r2 = rec[i];
          var tr = document.createElement("tr");
          var ts = new Date(r2.ts * 1000).toLocaleTimeString();
          tr.innerHTML = "<td>" + esc(ts) + "</td><td>" + esc(r2.model || "") + "</td><td>" +
            r2.prompt_tokens + "</td><td>" + r2.completion_tokens + "</td><td>" + r2.total_tokens +
            "</td><td>" + r2.elapsed_s + "s</td>";
          tb.appendChild(tr);
        }
      }
    }).catch(function () {});
  }
  function esc(s) {
    var d = document.createElement("div");
    d.textContent = String(s);
    return d.innerHTML;
  }

  var saveBtn = document.getElementById("s_save");
  if (saveBtn) saveBtn.addEventListener("click", function () {
    var body = {
      ollama_url: document.getElementById("s_ollama_url").value,
      model: (document.getElementById("s_model") || {}).value,
      input_price: parseFloat(document.getElementById("s_input_price").value),
      output_price: parseFloat(document.getElementById("s_output_price").value),
      usd_to_idr: parseFloat(document.getElementById("s_usd_to_idr").value)
    };
    fetch("/api/settings", {
      method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body)
    }).then(function (r) { return r.json(); }).then(function () {
      settings.ollama_url = body.ollama_url;
      currentModel = body.model;
      var lbl = document.getElementById("modelName");
      if (lbl) lbl.textContent = body.model;
      snack("Settings saved");
    }).catch(function () { snack("Save failed"); });
  });

  var testBtn = document.getElementById("s_test");
  if (testBtn) testBtn.addEventListener("click", function () {
    var st = document.getElementById("s_test_status");
    fetch("/api/settings/test", {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ ollama_url: document.getElementById("s_ollama_url").value })
    }).then(function (r) { return r.json(); }).then(function (d) {
      if (st) st.textContent = d.ok ? "OK: " + d.count + " models" : "FAIL: " + (d.error || "");
    }).catch(function () { if (st) st.textContent = "FAIL"; });
  });

  var exportBtn = document.getElementById("s_export");
  if (exportBtn) exportBtn.addEventListener("click", function () {
    window.location = "/stats/export";
  });

  // ---------- boot ----------
  renderHistory();
})();
