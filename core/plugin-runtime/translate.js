// This environment implements the documented translator API independently.
(function () {
  "use strict";
  const app = window.parent;
  const send = (message) => app.postMessage(message, "*");
  const replies = new Map();
  let nextCall = 1;
  let generation = 0;
  let selected = null;
  let catalog = [];
  const nativeTimeout = setTimeout;
  const nativeClearTimeout = clearTimeout;
  const CAP = 60000;

  function call(method, ...args) {
    const id = nextCall++;
    return new Promise((resolve, reject) => {
      const timer = nativeTimeout(() => {
        replies.delete(id);
        reject(new Error("Translator host did not answer in time."));
      }, CAP);
      replies.set(id, { resolve, reject, timer });
      send({ type: "call", id, method, args });
    });
  }

  function parseCode(code) {
    const text = String(code).replace(/^\uFEFF/, "").trimStart();
    if (text[0] !== "{") throw new Error("Translator has no JSON header.");
    let depth = 0;
    let quoted = false;
    let escaped = false;
    for (let i = 0; i < text.length; i++) {
      const c = text[i];
      if (quoted) {
        if (escaped) escaped = false;
        else if (c === "\\") escaped = true;
        else if (c === '"') quoted = false;
      } else if (c === '"') quoted = true;
      else if (c === "{") depth++;
      else if (c === "}" && --depth === 0) {
        return { header: JSON.parse(text.slice(0, i + 1)), body: text.slice(i + 1) };
      }
    }
    throw new Error("Translator header is incomplete.");
  }

  function matching(translators, url) {
    return translators.filter((translator) => {
      try { return new RegExp(translator.target || "").test(url); } catch { return false; }
    }).sort((a, b) => (a.priority ?? 100) - (b.priority ?? 100));
  }

  const documentTargets = new WeakMap();

  function documentAt(html, url, kind = "text/html") {
    const doc = new DOMParser().parseFromString(html, kind);
    const location = new URL(url);
    for (const key of ["URL", "documentURI"]) {
      try { Object.defineProperty(doc, key, { configurable: true, value: location.href }); }
      catch { /* The document view below also handles non-configurable properties. */ }
    }
    if (kind === "text/html") {
      let base = doc.querySelector("base");
      if (!base) {
        base = doc.createElement("base");
        doc.head.prepend(base);
      }
      base.setAttribute("href", new URL(base.getAttribute("href") || url, url).href);
    }
    // Native documents can have a non-configurable location accessor.
    // DOM methods keep their native receiver and unwrap document arguments.
    const methods = new Map();
    const view = new Proxy(doc, {
      get(target, key) {
        if (key === "location") return location;
        if (key === "URL" || key === "documentURI") return location.href;
        const value = Reflect.get(target, key, target);
        if (typeof value !== "function") return value;
        if (!methods.has(key)) methods.set(key, (...args) => value.apply(target, args.map((arg) => documentTargets.get(arg) || arg)));
        return methods.get(key);
      },
      set(target, key, value) { return Reflect.set(target, key, value, target); },
    });
    documentTargets.set(view, doc);
    return view;
  }

  const trim = (value) => String(value ?? "").trim();
  const trimInternal = (value) => trim(value).replace(/\s+/g, " ");
  function unescapeHTML(value) {
    return new DOMParser().parseFromString(String(value ?? ""), "text/html").documentElement.textContent || "";
  }
  const cleanTags = (value) => String(value ?? "").replace(/<[^>]*>/g, "");
  function cleanAuthor(value, creatorType = "author", hasComma = false) {
    const name = trimInternal(value).replace(/^[,;\s]+|[,;\s]+$/g, "");
    if (/[\u3400-\u9fff\u3040-\u30ff\uac00-\ud7af]/.test(name)) {
      return { lastName: name.replace(/\s+/g, ""), firstName: "", creatorType, fieldMode: 1 };
    }
    let firstName = "";
    let lastName = name;
    if (hasComma && name.includes(",")) {
      const parts = name.split(",");
      lastName = trim(parts.shift());
      firstName = trim(parts.join(","));
    } else {
      const parts = name.split(" ");
      lastName = parts.pop() || "";
      firstName = parts.join(" ");
    }
    return { firstName, lastName, creatorType };
  }

  function capitalizeTitle(value) {
    const text = trimInternal(value);
    const lower = text === text.toUpperCase() ? text.toLowerCase() : text;
    const minor = /^(a|an|and|as|at|but|by|for|in|nor|of|on|or|per|the|to|via|vs)$/i;
    return lower.replace(/\S+/g, (word, offset) => {
      if (offset > 0 && offset + word.length < lower.length && minor.test(word)) return word.toLowerCase();
      return word.replace(/^([\W_]*)(\p{L})/u, (_, prefix, letter) => prefix + letter.toUpperCase());
    });
  }

  function cleanISBN(value) {
    const candidates = String(value ?? "").match(/[\dXx][\dXx\s-]{8,}[\dXx]/g) || [];
    for (const candidate of candidates) {
      const isbn = candidate.replace(/[\s-]/g, "").toUpperCase();
      if (isbn.length === 10) {
        const sum = Array.from(isbn).reduce((total, c, i) => total + (c === "X" ? 10 : Number(c)) * (10 - i), 0);
        if (/^\d{9}[\dX]$/.test(isbn) && sum % 11 === 0) return isbn;
      } else if (/^97[89]\d{10}$/.test(isbn)) {
        const sum = Array.from(isbn).reduce((total, c, i) => total + Number(c) * (i % 2 ? 3 : 1), 0);
        if (sum % 10 === 0) return isbn;
      }
    }
    return false;
  }

  function cleanISSN(value) {
    const candidates = String(value ?? "").match(/\d{4}[ -]?\d{3}[\dXx]/g) || [];
    for (const candidate of candidates) {
      const issn = candidate.replace(/[ -]/g, "").toUpperCase();
      const sum = Array.from(issn).reduce((total, c, i) => total + (c === "X" ? 10 : Number(c)) * (8 - i), 0);
      if (sum % 11 === 0) return issn.slice(0, 4) + "-" + issn.slice(4);
    }
    return false;
  }

  function strToISO(value) {
    const text = trim(value);
    const numeric = text.match(/^(\d{4})(?:[-/.年](\d{1,2})(?:[-/.月](\d{1,2})日?)?)?$/);
    if (numeric) {
      if (numeric[2] && (+numeric[2] < 1 || +numeric[2] > 12)) return false;
      if (numeric[3] && (+numeric[3] < 1 || +numeric[3] > new Date(+numeric[1], +numeric[2], 0).getDate())) return false;
      return [numeric[1], numeric[2]?.padStart(2, "0"), numeric[3]?.padStart(2, "0")].filter(Boolean).join("-");
    }
    const date = new Date(text);
    return Number.isNaN(date.getTime()) ? false : date.toISOString().slice(0, 10);
  }

  function xpath(nodes, expression, namespaces) {
    return (Array.isArray(nodes) ? nodes : [nodes]).flatMap((value) => {
      const node = documentTargets.get(value) || value;
      const doc = node.ownerDocument || node;
      const resolver = namespaces ? (prefix) => namespaces[prefix] || null : null;
      const result = doc.evaluate(expression, node, resolver, 0, null);
      if (result.resultType === 2) return [result.stringValue];
      if (result.resultType === 1) return [String(result.numberValue)];
      if (result.resultType === 3) return [String(result.booleanValue)];
      const matches = [];
      let match;
      while ((match = result.iterateNext())) matches.push(match);
      return matches;
    });
  }

  const itemTypes = new Set(("artwork audioRecording bill blogPost book bookSection case computerProgram conferencePaper dataset dictionaryEntry document email encyclopediaArticle film forumPost hearing instantMessage interview journalArticle letter magazineArticle manuscript map newspaperArticle patent podcast preprint presentation radioBroadcast report standard statute thesis tvBroadcast videoRecording webpage").split(" "));
  const commonFields = new Set("title abstractNote date language shortTitle url accessDate archive archiveLocation libraryCatalog callNumber rights extra".split(" "));
  const typeFields = {
    journalArticle: "publicationTitle journalAbbreviation volume issue pages series seriesTitle seriesText DOI ISSN",
    magazineArticle: "publicationTitle volume issue pages ISSN",
    newspaperArticle: "publicationTitle place edition section pages ISSN",
    book: "publisher place edition volume numVolumes numberOfVolumes numPages numberOfPages series seriesNumber ISBN",
    bookSection: "bookTitle publisher place edition volume numVolumes numberOfVolumes pages series seriesNumber ISBN",
    conferencePaper: "proceedingsTitle conferenceName publisher place volume pages series DOI ISBN",
    thesis: "university thesisType place numPages numberOfPages",
    report: "institution reportType reportNumber place seriesTitle pages DOI",
    preprint: "repository archiveID type genre DOI",
    dataset: "repository repositoryLocation type version DOI",
    patent: "patentNumber applicationNumber assignee country place priorityNumbers issueDate filingDate legalStatus pages",
    webpage: "websiteTitle websiteType",
    blogPost: "blogTitle websiteType",
    forumPost: "forumTitle postType",
    document: "publisher place",
    encyclopediaArticle: "encyclopediaTitle publisher place edition volume pages series seriesNumber ISBN",
    dictionaryEntry: "dictionaryTitle publisher place edition volume pages series seriesNumber ISBN",
    artwork: "artworkMedium artworkSize place",
    audioRecording: "audioRecordingFormat label place runningTime seriesTitle volume numberOfVolumes ISBN",
    film: "distributor genre videoRecordingFormat runningTime",
    videoRecording: "studio place videoRecordingFormat runningTime volume numberOfVolumes ISBN",
    tvBroadcast: "programTitle episodeNumber network place runningTime videoRecordingFormat",
    radioBroadcast: "programTitle episodeNumber network place runningTime audioRecordingFormat",
    podcast: "seriesTitle episodeNumber audioRecordingFormat runningTime",
    computerProgram: "company place version system programmingLanguage",
    map: "mapType scale publisher place edition seriesTitle ISBN",
    presentation: "presentationType meetingName place",
    manuscript: "manuscriptType place numPages numberOfPages",
    standard: "organization publisher place number status DOI",
    interview: "interviewMedium",
    letter: "letterType",
    email: "subject",
    instantMessage: "subject",
    bill: "billNumber code codeVolume codePages legislativeBody session history",
    case: "caseName court reporter reporterVolume firstPage docketNumber history",
    hearing: "committee legislativeBody documentNumber session publisher place numPages",
    statute: "nameOfAct code codeNumber publicLawNumber pages section session history",
  };
  function fieldIsValidForType(field, type) {
    return itemTypes.has(type) && (commonFields.has(field) || (typeFields[type] || "").split(" ").includes(field));
  }

  function context(doc, url, output, input = "", depth = 0) {
    if (depth > 12) throw new Error("Translator nesting is too deep.");
    const tasks = new Set();
    const timers = new Map();
    const children = new Set();
    let failure = null;
    let active = true;
    let cursor = 0;
    let waiting = null;
    let finishWait = null;
    const ctx = { doc, url, output, depth };
    const check = () => { if (!active) throw new Error("Translation has ended."); };
    function track(promise, fatal = true) {
      check();
      const p = Promise.resolve(promise);
      const task = p.then(() => {}, (error) => { if (fatal) failure ||= error; }).finally(() => tasks.delete(task));
      tasks.add(task);
      return p;
    }
    function timeout(fn, delay, ...args) {
      check();
      let finish;
      const pending = new Promise((resolve) => { finish = resolve; });
      track(pending);
      const id = nativeTimeout(async () => {
        timers.delete(id);
        try { if (active) await fn(...args); } catch (e) { failure ||= e; }
        finally { finish(); }
      }, delay);
      timers.set(id, finish);
      return id;
    }
    function clearTimer(id) {
      nativeClearTimeout(id);
      timers.get(id)?.();
      timers.delete(id);
    }
    async function drain() {
      do {
        if (tasks.size) await Promise.all([...tasks]);
        for (const child of children) await child.drain();
        await new Promise((resolve) => nativeTimeout(resolve, 0));
        if (failure) throw failure;
      } while (tasks.size);
    }
    ctx.drain = drain;
    ctx.stop = () => {
      for (const child of children) child.stop();
      active = false;
      for (const id of timers.keys()) clearTimer(id);
    };
    ctx.run = async (fn, ...args) => {
      let timer;
      const limit = new Promise((_, reject) => { timer = nativeTimeout(() => reject(new Error("Translation exceeded 60 seconds.")), CAP); });
      try {
        await Promise.race([(async () => { await fn(...args); await drain(); })(), limit]);
      } finally { nativeClearTimeout(timer); }
    };
    ctx.track = track;

    function finishItem(item) {
      check();
      ctx.output(item);
    }
    class Item {
      constructor(itemType = "webpage") {
        this.itemType = itemType;
        this.creators = [];
        this.tags = [];
        this.notes = [];
        this.attachments = [];
      }
      complete() { finishItem(this); }
    }
    async function request(urlValue, options = {}) {
      check();
      const destination = new URL(urlValue, url).href;
      if (!/^https?:/.test(destination)) throw new Error("Only HTTP and HTTPS requests are available.");
      const response = await call("request", {
        url: destination, method: options.method || "GET", headers: options.headers || {},
        body: options.body == null ? null : String(options.body),
      });
      const allowed = options.successCodes;
      if (allowed !== false && !(Array.isArray(allowed) ? allowed.includes(response.status) : response.status >= 200 && response.status < 300)) {
        throw new Error("HTTP " + response.status + " from " + destination);
      }
      return response;
    }
    const requestText = (address, options) => track(request(address, options).then((r) => r.text), false);
    const requestJSON = (address, options) => track(request(address, options).then((r) => JSON.parse(r.text)), false);
    const requestDocument = (address, options) => track(request(address, options).then((r) => documentAt(r.text, r.url || new URL(address, url).href)), false);
    const nodeAt = (parent, selector, index = 0) => selector ? parent.querySelectorAll(selector)[index] : parent;
    const utilities = {
      xpath,
      xpathText: (nodes, expression, namespaces, delimiter = ", ") => xpath(nodes, expression, namespaces).map((n) => typeof n === "string" ? n : n.textContent || "").join(delimiter),
      text: (parent, selector, index) => trim(nodeAt(parent, selector, index)?.textContent),
      attr: (parent, selector, attribute, index) => nodeAt(parent, selector, index)?.getAttribute(attribute) || "",
      innerText: (parent, selector, index) => {
        const node = nodeAt(parent, selector, index);
        return trim(node?.innerText ?? node?.textContent);
      },
      trimInternal, trim, cleanAuthor, capitalizeTitle, cleanISBN, cleanISSN, strToISO,
      strToDate: (value) => {
        const iso = strToISO(value);
        if (!iso) return { part: trim(value) };
        const [year, month, day] = iso.split("-").map(Number);
        return { year, ...(month ? { month: month - 1 } : {}), ...(day ? { day } : {}) };
      },
      cleanDOI: (value) => String(value ?? "").match(/10\.\d{4,9}\/[^\s<>"]+/i)?.[0].replace(/[.,;]+$/, "") || false,
      unescapeHTML, cleanTags,
      superCleanString: (value) => trimInternal(value).replace(/^[\s\p{P}]+|[\s\p{P}]+$/gu, ""),
      itemTypeExists: (type) => itemTypes.has(type),
      fieldIsValidForType,
      getVersion: () => "7.0",
      requestText, requestJSON, requestDocument,
      request: (address, options = {}) => track(request(address, options).then((r) => ({
        status: r.status, headers: {}, body: options.responseType === "json" ? JSON.parse(r.text)
          : options.responseType === "document" ? documentAt(r.text, r.url) : r.text,
      })), false),
      doGet: (addresses, callback, done, charset) => track((async () => {
        for (const address of Array.isArray(addresses) ? addresses : [addresses]) {
          const r = await request(address, { responseCharset: charset });
          await callback(r.text, { status: r.status, responseText: r.text, responseURL: r.url }, r.url);
        }
        await done?.();
      })()),
      doPost: (address, body, callback, headers, charset) => track((async () => {
        const r = await request(address, { method: "POST", body, headers, responseCharset: charset });
        await callback?.(r.text, { status: r.status, responseText: r.text, responseURL: r.url });
      })()),
      processDocuments: (addresses, processor, done, onError) => track((async () => {
        try {
          for (const address of Array.isArray(addresses) ? addresses : [addresses]) {
            const r = await request(address);
            await processor(documentAt(r.text, r.url), r.url);
          }
          await done?.();
        } catch (error) { if (onError) await onError(error); else throw error; }
      })()),
    };
    utilities.HTTP = {
      request: (method, address, options = {}) => track(request(address, { ...options, method }).then((r) => ({
        status: r.status, responseText: r.text, responseURL: r.url,
        response: options.responseType === "json" ? JSON.parse(r.text)
          : options.responseType === "document" ? documentAt(r.text, r.url) : r.text,
        getResponseHeader: () => null,
      })), false),
      doGet: utilities.doGet, doPost: utilities.doPost, processDocuments: utilities.processDocuments,
    };

    const Zotero = {
      Item,
      debug: (...values) => console.debug(...values),
      logError: (error) => console.debug(error),
      getHiddenPref: () => undefined,
      getOption: () => undefined,
      getVersion: utilities.getVersion,
      itemTypeExists: utilities.itemTypeExists,
      fieldIsValidForType: utilities.fieldIsValidForType,
      Utilities: utilities,
      selectItems: (items, callback) => track((async () => {
        const keys = await call("selectItems", items);
        const chosen = Object.create(null);
        for (const key of Array.isArray(keys) ? keys : []) {
          if (Object.prototype.hasOwnProperty.call(items, key)) chosen[key] = items[key];
        }
        const result = Object.keys(chosen).length ? chosen : false;
        await callback?.(result);
        return result;
      })()),
      wait: () => {
        if (!waiting) {
          waiting = new Promise((resolve) => { finishWait = resolve; });
          track(waiting);
        }
      },
      done: () => { finishWait?.(); waiting = null; finishWait = null; },
      read: (length) => {
        if (cursor >= input.length) return false;
        if (length !== undefined) {
          const end = Math.min(input.length, cursor + Math.max(0, Number(length)));
          const chunk = input.slice(cursor, end);
          cursor = end;
          return chunk;
        }
        const end = input.indexOf("\n", cursor);
        const line = input.slice(cursor, end < 0 ? input.length : end).replace(/\r$/, "");
        cursor = end < 0 ? input.length : end + 1;
        return line;
      },
      nextItem: () => {
        const line = Zotero.read();
        if (line === false) return false;
        try { return JSON.parse(line); } catch { return line; }
      },
      getXML: () => new DOMParser().parseFromString(input, "application/xml"),
      setProgress: () => {},
      configure: () => {},
      addOption: () => {},
    };
    if (depth === 0) Object.assign(window, { Zotero, Z: Zotero, ZU: utilities });
    ctx.resetReader = () => { cursor = 0; };
    ctx.Zotero = Zotero;
    ctx.utilities = utilities;
    ctx.timeout = timeout;
    ctx.clearTimer = clearTimer;

    Zotero.loadTranslator = (type) => {
      let translatorId = null;
      let childDoc = doc;
      let childString = "";
      let loaded = null;
      const handlers = new Map();
      const childOutput = (item) => {
        // The caller completes an item after its itemDone edits, or it is
        // forwarded automatically when no handler was registered.
        item.complete = () => finishItem(item);
        const handler = handlers.get("itemDone");
        if (handler) track(Promise.resolve().then(() => handler(api, item)));
        else finishItem(item);
      };
      async function load(id) {
        if (loaded?.header.translatorID === id) return loaded;
        const child = context(childDoc, childDoc?.URL || url, childOutput, childString, depth + 1);
        children.add(child);
        loaded = await loadCode(id, child);
        return loaded;
      }
      const api = {
        setTranslator: (id) => {
          const value = Array.isArray(id) ? id[0] : id;
          translatorId = typeof value === "string" ? value : value?.translatorID || value?.id;
          loaded = null;
        },
        setDocument: (value) => { childDoc = value; loaded = null; },
        setString: (value) => { childString = String(value); loaded = null; },
        setHandler: (name, fn) => handlers.set(name, fn),
        getTranslators: () => track((async () => {
          const importing = type === "import" || !!childString;
          const candidates = translatorId ? [{ id: translatorId }] : importing ? await call("translators") : matching(catalog, childDoc?.URL || url);
          const found = [];
          for (const candidate of candidates) {
            try {
              const script = await load(candidate.id);
              if (!(script.header.translatorType & (importing ? 1 : 4))) continue;
              const fn = importing ? script.functions.detectImport : script.functions.detectWeb;
              let detected;
              await script.ctx.run(async () => { detected = await fn?.(childDoc, childDoc?.URL || url); });
              script.ctx.resetReader();
              if (detected) found.push({ ...script.header, id: candidate.id });
            } catch (error) { console.debug(error); }
          }
          found.sort((a, b) => a.priority - b.priority);
          await handlers.get("translators")?.(api, found);
          return found;
        })()),
        getTranslatorObject: (callback) => track((async () => {
          if (!translatorId) throw new Error("No nested translator selected.");
          const script = await load(translatorId);
          if (callback) await callback(script.functions.object);
          return script.functions.object;
        })()),
        translate: () => track((async () => {
          try {
            if (!translatorId) {
              const candidates = await api.getTranslators();
              if (!candidates.length) throw new Error("No nested translator detected.");
              translatorId = candidates[0].translatorID;
            }
            const script = await load(translatorId);
            script.ctx.resetReader();
            const importing = type === "import" || !!childString;
            const fn = importing ? script.functions.doImport : script.functions.doWeb;
            if (!fn) throw new Error("Translator has no " + (importing ? "doImport" : "doWeb") + ".");
            await script.ctx.run(fn, childDoc, childDoc?.URL || url);
            await handlers.get("done")?.(api, true);
          } catch (error) {
            await handlers.get("error")?.(api, error);
            throw error;
          }
        })()),
      };
      return api;
    };
    return ctx;
  }

  async function loadCode(id, ctx) {
    const parsed = parseCode(await call("translatorCode", id));
    const utilities = ctx.utilities;
    const environment = {
      Zotero: ctx.Zotero, Z: ctx.Zotero, ZU: utilities, ZOTERO_TRANSLATOR_INFO: parsed.header,
      setTimeout: ctx.timeout, clearTimeout: ctx.clearTimer,
    };
    for (const key of ["text", "attr", "innerText", "requestText", "requestDocument", "requestJSON", "request"]) environment[key] = utilities[key];
    const factory = new Function("environment", "with (environment) { return (function () {\n" + parsed.body + `
      return (function () {
      const functions = {
        detectWeb: typeof detectWeb === "function" ? detectWeb : null,
        doWeb: typeof doWeb === "function" ? doWeb : null,
        detectImport: typeof detectImport === "function" ? detectImport : null,
        doImport: typeof doImport === "function" ? doImport : null
      };
      functions.object = new Proxy(functions, {
        get(target, key) {
          if (key in target) return target[key];
          if (typeof key !== "string" || !/^[a-zA-Z_$][\\w$]*$/.test(key)) return undefined;
          try { return eval(key); } catch { return undefined; }
        },
        set(target, key, value) {
          if (typeof key !== "string" || !/^[a-zA-Z_$][\\w$]*$/.test(key)) return false;
          try { eval(key + " = value"); } catch { target[key] = value; }
          return true;
        }
      });
      return functions;
      })();
    ` + "\n}).call(environment); }");
    const functions = factory(environment);
    return { ...parsed, ctx, functions };
  }

  function itemJSON(item) {
    const plain = {};
    for (const [key, value] of Object.entries(item)) {
      if (["complete", "notes", "attachments"].includes(key) || typeof value === "function") continue;
      plain[key] = value;
    }
    plain.attachments = (item.attachments || []).map((a) => ({
      url: a.url || a.document?.URL, mimeType: a.mimeType, title: a.title,
    }));
    return JSON.parse(JSON.stringify(plain));
  }

  window.addEventListener("message", async (event) => {
    if (event.source !== app || !event.data || typeof event.data !== "object") return;
    const message = event.data;
    if (message.type === "reply") {
      const pending = replies.get(message.id);
      if (!pending) return;
      replies.delete(message.id);
      nativeClearTimeout(pending.timer);
      if (message.error) pending.reject(new Error(message.error));
      else pending.resolve(message.value);
      return;
    }
    const id = message.id;
    try {
      if (message.type === "translate") {
        const current = ++generation;
        selected?.ctx.stop();
        selected = null;
        catalog = message.translators || [];
        const doc = documentAt(message.html, message.url);
        const candidates = matching(catalog, message.url);
        for (const translator of candidates) {
          const ctx = context(doc, message.url, () => {});
          try {
            const script = await loadCode(translator.id, ctx);
            let itemType;
            await ctx.run(async () => { itemType = await script.functions.detectWeb?.(doc, message.url); });
            if (current !== generation) { ctx.stop(); return; }
            if (itemType) {
              selected = { translator, doc, url: message.url, ctx, script, used: false };
              send({ type: "detected", id, translator: translator.id, label: translator.label, itemType });
              return;
            }
          } catch (error) { console.debug("Translator detection failed", translator.label, error); }
          ctx.stop();
        }
        if (current === generation) send({ type: "detected", id, translator: null });
      } else if (message.type === "save") {
        if (!selected) throw new Error("No translator detected for this page.");
        const current = generation;
        const items = [];
        const completed = new WeakSet();
        const choice = selected;
        const { translator, doc, url } = choice;
        let ctx = choice.ctx;
        let script = choice.script;
        const output = (item) => {
          if (!completed.has(item)) { completed.add(item); items.push(itemJSON(item)); }
        };
        try {
          if (choice.used) {
            ctx = context(doc, url, output);
            choice.ctx = ctx;
            script = await loadCode(translator.id, ctx);
            choice.script = script;
            await ctx.run(async () => { await script.functions.detectWeb?.(doc, url); });
          }
          choice.used = true;
          ctx.output = output;
          if (!script.functions.doWeb) throw new Error("Translator has no doWeb.");
          await ctx.run(script.functions.doWeb, doc, url);
          if (current === generation) send({ type: "translated", id, items });
        } finally { ctx.stop(); }
      }
    } catch (error) {
      send({ type: "translate-error", id, message: error instanceof Error ? error.message : String(error) });
    }
  });
  send({ type: "loaded" });
})();
