// The Zotero 7 plugin runtime: runs one bootstrap plugin inside a sandboxed
// frame of its own and gives it the `Zotero` object (and the parts of
// `Services` plugins lean on). The library's items come from the app in
// Zotero's JSON shape; what a plugin changes goes back to the app as a
// message, which the app may refuse.
//
// Messages from the app: start, reply, items, settings, settings-close,
// render-section, column-values, reader-render, reader-release, reader-event,
// reader-menu, reader-command,
// item-menu, item-command, stop. Messages to the app: call (answered
// by a reply), loaded, ready, error, partial, section, column,
// column-removed, column-values, reader-events, reader-rendered, reader-menu,
// item-menu, settings-available, show, hide, rendered.
// The same file serves the Tauri app and the Apple apps.

(() => {
  "use strict";

  const app_ = window.parent;
  let nextCall = 1;
  const waiting = new Map();
  const send = (message) => app_.postMessage(message, "*");

  function call(method, ...args) {
    const id = nextCall++;
    return new Promise((resolve, reject) => {
      waiting.set(id, { resolve, reject });
      send({ type: "call", id, method, args });
    });
  }

  let itemMenuErrorReported = null;
  function report(error) {
    if (itemMenuErrorReported !== null) {
      if (itemMenuErrorReported) return;
      itemMenuErrorReported = true;
    }
    send({ type: "error", message: describe(error) });
  }

  // WebKit's stacks list frames only, without the message itself.
  function describe(error) {
    if (!error || !error.stack) return String(error);
    const stack = String(error.stack);
    return stack.includes(String(error.message)) ? stack : `${error.name || "Error"}: ${error.message}\n${stack}`;
  }
  window.addEventListener("error", (event) => report(event.error || event.message));
  window.addEventListener("unhandledrejection", (event) => {
    report(event.reason);
  });

  const unsupported = new Set();
  function partial(what) {
    if (unsupported.has(what)) return;
    unsupported.add(what);
    send({ type: "partial", what });
  }

  // A sandboxed frame has no storage of its own; one that lasts while the
  // plugin runs.
  const memory = new Map();
  const memoryStorage = {
    getItem: (key) => (memory.has(String(key)) ? memory.get(String(key)) : null),
    setItem: (key, value) => memory.set(String(key), String(value)),
    removeItem: (key) => memory.delete(String(key)),
    clear: () => memory.clear(),
    key: (index) => [...memory.keys()][index] ?? null,
    get length() {
      return memory.size;
    },
  };
  for (const name of ["localStorage", "sessionStorage"]) {
    try {
      Object.defineProperty(window, name, { value: memoryStorage, configurable: true });
    } catch {
      // Left as it is.
    }
  }

  // The plugin's files, by path; given all at once so scripts load at once.
  const ROOT = "plugin://zotero/";
  const files = new Map();
  // Chrome packages a plugin registers (registerChrome, as Zotero 7
  // bootstraps do): chrome://<name>/content/… is that folder of the plugin.
  const chromePackages = new Map();
  function fileText(url) {
    let text = String(url);
    const packaged = text.match(/^chrome:\/\/([^/]+)\/content\/(.*)$/);
    if (packaged && chromePackages.has(packaged[1])) text = chromePackages.get(packaged[1]) + packaged[2];
    const path = text
      .replace(ROOT, "")
      .replace(/^chrome:\/\/[^/]+\/content\//, "content/")
      .replace(/^\/+/, "").split("/").reduce((parts, part) => {
        if (part === "..") parts.pop();
        else if (part && part !== ".") parts.push(part);
        return parts;
      }, []).join("/");
    if (!files.has(path)) throw new Error(`${url} is not in the plugin`);
    return files.get(path);
  }

  // Fluent strings from the plugin's English locale: `id = value` and
  // `.label = value` under an id.
  const strings = new Map();
  function readLocales() {
    for (const [path, text] of files) {
      if (!/locale\/en-US\/.*\.ftl$/.test(path)) continue;
      let current = null;
      let valueKey = null;
      for (const line of text.split("\n")) {
        const attribute = /^\s+\.(\w+)\s*=\s*(.*)$/.exec(line);
        const entry = /^([\w-]+)\s*=\s*(.*)$/.exec(line);
        if (entry) {
          current = entry[1];
          valueKey = current;
          if (entry[2].trim()) strings.set(current, entry[2].trim());
        } else if (attribute && current) {
          valueKey = `${current}.${attribute[1]}`;
          strings.set(valueKey, attribute[2].trim());
        } else if (valueKey && /^\s+\S/.test(line)) {
          strings.set(valueKey, [strings.get(valueKey), line.trim()].filter(Boolean).join("\n"));
        } else valueKey = null;
      }
    }
  }
  const label = (id, fallback) => strings.get(`${id}.label`) ?? strings.get(id) ?? fallback ?? id;

  // Preferences, kept by the app for this plugin.
  let prefs = {};
  const defaultPrefs = Object.create(null);
  const owns = (object, key) => Object.prototype.hasOwnProperty.call(object, key);
  // Keep saved keys from earlier runtimes usable; Zotero-relative names
  // can also address the fully qualified names in packaged defaults.
  const prefKey = (key, global) => !global && !owns(prefs, key) && !owns(defaultPrefs, key)
    ? `extensions.zotero.${key}` : String(key);
  const prefValue = (key) => owns(prefs, key) ? prefs[key] : defaultPrefs[key];
  function readDefaultPrefs() {
    for (const line of (files.get("prefs.js") || "").split(/\r?\n/)) {
      const match = /^\s*pref\(\s*("(?:[^"\\]|\\.)*")\s*,\s*("(?:[^"\\]|\\.)*"|true|false|-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?)\s*\)\s*;?\s*(?:\/\/.*)?$/.exec(line);
      if (!match) continue;
      try {
        defaultPrefs[JSON.parse(match[1])] = JSON.parse(match[2]);
      } catch {
        // Invalid JSON escapes are not preference declarations.
      }
    }
  }
  const savePrefs = () => call("prefs.save", JSON.stringify(prefs)).catch(report);
  const prefObservers = new Map();
  let nextPrefObserver = 1;
  const prefChanged = (key) => {
    for (const { name, handler } of [...prefObservers.values()]) {
      if (name !== key) continue;
      try {
        handler(prefValue(key));
      } catch (error) {
        report(error);
      }
    }
  };
  const Prefs = {
    get: (key, global) => prefValue(prefKey(key, global)),
    set: (key, value, global) => {
      key = prefKey(key, global);
      prefs[key] = value;
      savePrefs();
      prefChanged(key);
      return value;
    },
    clear: (key, global) => {
      key = prefKey(key, global);
      delete prefs[key];
      savePrefs();
      prefChanged(key);
    },
    resetBranch(names = [], prefix = "extensions.zotero.") {
      for (const key of Object.keys(prefs)) {
        if (key.startsWith(prefix) && !names.includes(key.slice(prefix.length))) Prefs.clear(key, true);
      }
    },
    registerObserver(name, handler, global) {
      const id = nextPrefObserver++;
      prefObservers.set(id, { name: prefKey(name, global), handler });
      return id;
    },
    unregisterObserver: (id) => prefObservers.delete(id),
  };

  // Static Zotero schema for the regular item types supplied by Paperloom.
  // Field/base-field and creator relationships: Zotero 7.0.32's global schema
  // (zotero-schema 7f04bb5a6c0c8acfb070849cec81775d1ac25947).
  // IDs are frame-local registry IDs, never host database IDs.
  const schema = {
    journalArticle: ["title abstractNote publicationTitle publisher place date volume issue section partNumber partTitle pages series seriesTitle seriesText journalAbbreviation DOI citationKey url accessDate PMID PMCID ISSN archive archiveLocation shortTitle language libraryCatalog callNumber rights extra", "author contributor editor translator reviewedAuthor"],
    conferencePaper: ["title abstractNote proceedingsTitle:publicationTitle conferenceName publisher place date eventPlace volume issue numberOfVolumes pages series seriesNumber DOI ISBN citationKey url accessDate ISSN archive archiveLocation shortTitle language libraryCatalog callNumber rights extra", "author contributor editor translator seriesEditor"],
    book: ["title abstractNote series seriesNumber volume numberOfVolumes edition date publisher place originalDate originalPublisher originalPlace format:medium numPages ISBN DOI citationKey url accessDate ISSN archive archiveLocation shortTitle language libraryCatalog callNumber rights extra", "author contributor editor translator seriesEditor"],
    bookSection: ["title abstractNote bookTitle:publicationTitle series seriesNumber volume numberOfVolumes edition date publisher place originalDate originalPublisher originalPlace format:medium pages ISBN DOI citationKey url accessDate ISSN archive archiveLocation shortTitle language libraryCatalog callNumber rights extra", "author contributor editor bookAuthor translator seriesEditor"],
    thesis: ["title abstractNote thesisType:type university:publisher place date series seriesNumber numPages DOI ISBN citationKey url accessDate ISSN archive archiveLocation shortTitle language libraryCatalog callNumber rights extra", "author contributor"],
    report: ["title abstractNote reportNumber:number reportType:type institution:publisher place date seriesTitle seriesNumber pages DOI ISBN citationKey url accessDate ISSN archive archiveLocation shortTitle language libraryCatalog callNumber rights extra", "author editor contributor translator seriesEditor"],
    preprint: ["title abstractNote genre:type repository:publisher archiveID:number place date series seriesNumber DOI citationKey url accessDate archive archiveLocation shortTitle language libraryCatalog callNumber rights extra", "author contributor editor translator reviewedAuthor"],
    patent: ["title abstractNote place country assignee issuingAuthority:authority patentNumber:number filingDate pages applicationNumber priorityNumbers issueDate:date priorityDate:originalDate references legalStatus:status DOI citationKey url accessDate shortTitle language rights extra", "inventor attorneyAgent contributor"],
    standard: ["title abstractNote organization:authority committee type number versionNumber edition status date publisher place partNumber partTitle ISBN DOI citationKey url accessDate archive archiveLocation shortTitle numPages language libraryCatalog callNumber rights extra", "author editor contributor"],
    dataset: ["title abstractNote identifier:number type versionNumber date repository:publisher repositoryLocation:place format:medium DOI citationKey url accessDate archive archiveLocation shortTitle language libraryCatalog callNumber rights extra", "author contributor"],
    webpage: ["title abstractNote websiteTitle:publicationTitle websiteType:type date publisher place DOI citationKey url accessDate shortTitle language rights extra", "author contributor translator"],
    document: ["title abstractNote type date publisher place DOI citationKey url accessDate archive archiveLocation shortTitle language libraryCatalog callNumber rights extra", "author contributor editor translator reviewedAuthor"],
  };
  function registry(names) {
    const entries = [...new Set(names)].map((name, index) => ({ id: index + 1, name }));
    const ids = new Map(entries.map(({ id, name }) => [name, id]));
    return {
      getID: (value) => typeof value === "number" ? (entries[value - 1]?.id || false) : (ids.get(value) || false),
      getName: (value) => typeof value === "number" ? (entries[value - 1]?.name || false) : (ids.has(value) ? value : false),
      getAll: () => entries.map((entry) => ({ ...entry })),
      getLocalizedString: (value) => typeof value === "number" ? (entries[value - 1]?.name || "") : (ids.has(value) ? value : ""),
    };
  }
  const typeFields = new Map(Object.entries(schema).map(([name, [fields]]) => [
    name, fields.split(" ").map((field) => { const [name, base = name] = field.split(":"); return { name, base }; }),
  ]));
  const ItemTypes = registry([...Object.keys(schema), "attachment", "annotation"]);
  const ItemFields = registry([...typeFields.values()].flatMap((fields) => fields.flatMap(({ name, base }) => [name, base])));
  const CreatorTypes = registry(Object.values(schema).flatMap(([, creators]) => creators.split(" ")));
  const fieldsForType = (type) => typeFields.get(ItemTypes.getName(type)) || [];
  Object.assign(ItemFields, {
    isValidForType: (field, type) => fieldsForType(type).some(({ name }) => name === ItemFields.getName(field)),
    getItemTypeFields: (type) => fieldsForType(type).map(({ name }) => ItemFields.getID(name)),
    getTypeFieldsFromBase(base, asNames = false) {
      base = ItemFields.getName(base);
      const names = [...new Set([...typeFields.values()].flatMap((fields) =>
        fields.filter((field) => field.base === base && field.name !== base).map((field) => field.name)))];
      return asNames ? names : names.map(ItemFields.getID);
    },
    getFieldIDFromTypeAndBase(type, base) {
      const field = fieldsForType(type).find((field) => field.base === ItemFields.getName(base));
      return field ? ItemFields.getID(field.name) : false;
    },
    getBaseIDFromTypeAndField(type, field) {
      const entry = fieldsForType(type).find((entry) => entry.name === ItemFields.getName(field));
      return entry ? ItemFields.getID(entry.base) : false;
    },
  });
  ItemTypes.getTypesForField = (field) => ItemTypes.getAll().filter(({ id }) => ItemFields.isValidForType(field, id)).map(({ id }) => id);
  CreatorTypes.getTypesForItemType = (type) => (schema[ItemTypes.getName(type)]?.[1] || "").split(" ").filter(Boolean)
    .map((name) => ({ id: CreatorTypes.getID(name), name }));
  CreatorTypes.getPrimaryIDForType = (type) => CreatorTypes.getTypesForItemType(type)[0]?.id || false;
  const baseFieldName = (type, field) => {
    field = ItemFields.getName(field) || String(field);
    return fieldsForType(type).find((entry) => entry.name === field)?.base || field;
  };

  const padDate = (value, length = 2) => String(value).padStart(length, "0");
  function isoDate(text) {
    text = String(text ?? "").trim();
    if (!text) return false;
    let match = /^(\d{4})(?:[-/.](\d{1,2})(?:[-/.](\d{1,2}))?)?$/.exec(text);
    if (!match) {
      // English month names and US numeric dates; avoid machine-dependent parsing.
      const months = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
      const words = text.replace(/,/g, "").split(/\s+/);
      const monthIndex = words.findIndex((word) => months.includes(word.slice(0, 3).toLowerCase()));
      const year = words.find((word) => /^\d{4}$/.test(word));
      const day = words.find((word, index) => index !== monthIndex && /^\d{1,2}(?:st|nd|rd|th)?$/i.test(word));
      if (monthIndex >= 0 && year && words.length <= 3) {
        match = [text, year, String(months.indexOf(words[monthIndex].slice(0, 3).toLowerCase()) + 1), day ? String(parseInt(day, 10)) : undefined];
      } else {
        const numeric = /^(\d{1,2})[/.\-](\d{1,2})[/.\-](\d{4})$/.exec(text);
        if (numeric) match = [text, numeric[3], numeric[1], numeric[2]];
      }
    }
    if (!match) return false;
    const [, year, month, day] = match;
    if (Number(year) < 1 || (month && (Number(month) < 1 || Number(month) > 12))) return false;
    if (day && (Number(day) < 1 || Number(day) > new Date(Date.UTC(Number(year), Number(month), 0)).getUTCDate())) return false;
    return year + (month ? "-" + padDate(Number(month)) : "") + (day ? "-" + padDate(Number(day)) : "");
  }
  const ZoteroDate = {
    strToISO: isoDate,
    strToDate(text) {
      const iso = isoDate(text);
      if (!iso) return { part: String(text ?? "") };
      const [year, month, day] = iso.split("-").map(Number);
      return { year, ...(month ? { month: month - 1 } : {}), ...(day ? { day } : {}), part: "" };
    },
    dateToSQL(date, toUTC = false) {
      if (!date || !Number.isFinite(date.getTime())) return false;
      const part = (name) => date[(toUTC ? "getUTC" : "get") + name]();
      return `${padDate(part("FullYear"), 4)}-${padDate(part("Month") + 1)}-${padDate(part("Date"))} ${padDate(part("Hours"))}:${padDate(part("Minutes"))}:${padDate(part("Seconds"))}`;
    },
    sqlToDate(text, isUTC = false) {
      if (!/^\d{4}-\d{2}-\d{2}(?: \d{2}:\d{2}:\d{2})?$/.test(String(text))) return false;
      const [year, month, day, hour = 0, minute = 0, second = 0] = String(text).split(/[- :]/).map(Number);
      if (!isoDate(`${year}-${month}-${day}`) || hour > 23 || minute > 59 || second > 59) return false;
      return isUTC ? new Date(Date.UTC(year, month - 1, day, hour, minute, second)) : new Date(year, month - 1, day, hour, minute, second);
    },
    isSQLDate: (text, allowZeroes = false) => /^\d{4}-\d{2}-\d{2}$/.test(String(text))
      && (!!isoDate(text) || (allowZeroes && /^\d{4}-(?:00|0[1-9]|1[0-2])-00$/.test(text))),
  };
  function capitalizeName(text) {
    if (typeof text !== "string") return text;
    return text.split(" ").map((part) => part === part.toLowerCase() || part === part.toUpperCase()
      ? part.toLowerCase().replace(/(^|[^\p{L}])\p{L}/gu, (letter) => letter.toUpperCase()) : part).join(" ");
  }
  function sentenceCase(text) {
    text = String(text ?? "");
    const plain = text.replace(/<[^>]+>/g, "");
    const allCaps = plain === plain.toUpperCase();
    // Preserve markup, explicit nocase spans, acronyms and mixed-case names.
    let start = true;
    return text.replace(/<span class=["']nocase["']>[\s\S]*?<\/span>|<nc>[\s\S]*?<\/nc>|<[^>]+>|[\p{L}\p{N}]+|[.?!]\s+/gu, (word) => {
      if (word.startsWith("<")) return word;
      if (/^[.?!]/.test(word)) { start = true; return word; }
      const first = start;
      start = false;
      if (!allCaps && (/^.\p{Lu}/u.test(word) || /^\p{Lu}+$/u.test(word) || /\d/.test(word))) return word;
      const lower = word.toLowerCase();
      return first && /^\p{Lu}/u.test(word) ? lower[0].toUpperCase() + lower.slice(1) : lower;
    });
  }
  function capitalizeTitle(text, force = false) {
    text = String(text ?? "").trim();
    if (force || text === text.toUpperCase()) text = text.toLowerCase();
    const small = /^(a|an|and|as|at|but|by|for|in|nor|of|on|or|per|the|to|vs\.?|via)$/i;
    let index = 0;
    return text.replace(/[\p{L}\p{N}][\p{L}\p{N}'’]*/gu, (word, offset) => {
      const first = index++ === 0;
      const last = !/[\p{L}\p{N}]/u.test(text.slice(offset + word.length));
      return !first && !last && small.test(word) ? word.toLowerCase() : word[0].toUpperCase() + word.slice(1);
    });
  }
  function cleanISBN(text, dontValidate = false) {
    const candidates = String(text ?? "").toUpperCase().replace(/[-\u00ad\u2010-\u2015\u2212]/g, "")
      .match(/\b(?:97[89]\s*(?:\d\s*){9}\d|(?:\d\s*){9}[\dX])\b/g) || [];
    for (const candidate of candidates) {
      const digits = candidate.replace(/\s/g, "");
      const sum = [...digits].reduce((total, digit, index) =>
        total + (digit === "X" ? 10 : Number(digit)) * (digits.length === 10 ? 10 - index : index % 2 ? 3 : 1), 0);
      if (dontValidate || sum % (digits.length === 10 ? 11 : 10) === 0) return digits;
    }
    return false;
  }

  // The library's items.
  let byKey = new Map();
  let byId = new Map();
  let nextItemID = 1;
  const itemIDs = new Map();
  let persistentKeys = [];
  const selections = [];
  let itemsSeq = -1;
  let selectionSeq = -1;
  let notifierSeq = -1;
  let tabsSeq = -1;
  const pane = {
    getSelectedItems: (asIDs = false) => (selections.at(-1)?.keys || persistentKeys).map((key) => byKey.get(key)).filter(Boolean)
      .map((item) => asIDs ? item.id : item),
    selectItem(id) { return this.selectItems([id]); },
    async selectItems(ids) {
      const keys = ids.map((id) => byId.get(id)?.key);
      if (keys.some((key) => !key)) throw new Error("Cannot select an unknown item");
      return call("pane.selectItems", keys);
    },
    document,
    // Plugins can look for title editors during startup; this frame has none.
    itemPane: { _itemDetails: { _header: null, querySelectorAll: () => [] } },
  };
  class Item {
    constructor(data, id) {
      // new Zotero.Item("note"): a note a plugin is making, saved on saveTx.
      if (typeof data === "string") data = { itemType: data, key: null, _new: true };
      this.id = id;
      this.key = data.key;
      this.libraryID = 1;
      this._itemType = data.itemType || "document";
      this._deleted = !!data.deleted;
      this._data = { ...data };
      this._savedFields = { ...data };
      this._creators = (data.creators || []).map((creator) => this._creator(creator));
      this._savedCreators = JSON.stringify(this._creators);
      this._changed = {};
      this._tags = (data.tags || []).map((t) => t.tag);
      this._tagsChanged = false;
      this._saveTail = null;
      this._savingFields = null;
    }
    _install(data) {
      const creatorsDirty = this._savingFields || JSON.stringify(this._creators) !== this._savedCreators;
      this._itemType = data.itemType || "document";
      this._deleted = !!data.deleted;
      this._savedFields = { ...data };
      this._data = { ...data, ...this._savingFields, ...this._changed };
      const creators = (data.creators || []).map((creator) => this._creator(creator));
      if (!creatorsDirty) this._creators = creators;
      this._savedCreators = JSON.stringify(creators);
      if (!this._tagsChanged) this._tags = (data.tags || []).map((tag) => tag.tag);
    }
    get itemType() { return this._itemType; }
    set itemType(value) { this._assertWritable("itemType"); this._itemType = value; }
    get deleted() { return this._deleted; }
    set deleted(value) { this._assertWritable("deleted"); this._deleted = value; }
    get itemTypeID() {
      return ItemTypes.getID(this.itemType);
    }
    getField(field, _unformatted, includeBaseMapped = false) {
      if (includeBaseMapped) field = ItemFields.getFieldIDFromTypeAndBase(this.itemTypeID, field) || field;
      field = baseFieldName(this.itemTypeID, field);
      if (field === "firstCreator") {
        const first = this._creators[0];
        return first ? first.lastName || first.name || "" : "";
      }
      const value = this._data[field];
      return value === undefined || value === null ? "" : String(value);
    }
    _assertWritable(field) {
      // Plugins make notes and change their own; the person's notes are theirs.
      if (this.isNote()) {
        const parent = ["parentKey", "parentItemKey", "parentID", "parentItemID", "parentItem"].includes(field);
        if (this._data._new ? field === "note" || parent : field === "note" && this._data.paperloomPluginNote) return;
        throw new Error(this._data._new || this._data.paperloomPluginNote
          ? `Changing a note's ${field} is not supported`
          : "Plugins can change only the notes they made");
      }
      if (!this.isNote() && this._data._new) throw new Error("Only new notes can be made by plugins");
      if (this.isAnnotation() && !["annotationComment", "annotationColor", "tags"].includes(field)) {
        throw new Error(`Annotation field ${field} writes are not supported`);
      }
    }
    setField(field, value) {
      field = baseFieldName(this.itemTypeID, field);
      this._assertWritable(field);
      if (this.isAnnotation() && typeof value !== "string") throw new Error(`Invalid annotation field ${field}`);
      value = value === false || value == null ? "" : String(value);
      if (this.getField(field) === value) return false;
      this._data[field] = value;
      const saved = this._savingFields && field in this._savingFields ? this._savingFields[field] : this._savedFields[field];
      if (String(saved ?? "") === value) delete this._changed[field];
      else this._changed[field] = value;
      return true;
    }
    isFieldOfBase(field, base) {
      return baseFieldName(this.itemTypeID, field) === (ItemFields.getName(base) || base);
    }
    getDisplayTitle() {
      return this.getField("title");
    }
    getCreators() {
      return this._creators.map((creator) => ({ ...creator }));
    }
    getCreatorsJSON() {
      return this._creators.map(({ firstName, lastName, fieldMode, creatorTypeID }) => ({
        ...(fieldMode === 1 ? { name: lastName } : { firstName, lastName }),
        creatorType: CreatorTypes.getName(creatorTypeID),
      }));
    }
    _creator(creator) {
      const fieldMode = creator.name !== undefined || creator.fieldMode === 1 ? 1 : 0;
      return {
        firstName: fieldMode ? "" : String(creator.firstName || ""),
        lastName: String(creator.name ?? creator.lastName ?? ""),
        fieldMode,
        creatorTypeID: CreatorTypes.getID(creator.creatorTypeID ?? creator.creatorType) || CreatorTypes.getPrimaryIDForType(this.itemTypeID),
      };
    }
    setCreators(creators) {
      this._assertWritable("creators");
      const next = creators.map((creator) => this._creator(creator));
      if (JSON.stringify(next) === JSON.stringify(this._creators)) return false;
      this._creators = next;
      return true;
    }
    setCreator(index, creator) {
      if (!Number.isInteger(index) || index < 0 || index > this._creators.length) throw new Error("Invalid creator index");
      const creators = this.getCreators();
      creators[index] = creator;
      return this.setCreators(creators);
    }
    getCreator(index) {
      return this._creators[index] ? { ...this._creators[index] } : false;
    }
    getUsedFields() {
      return ItemFields.getItemTypeFields(this.itemTypeID).filter((field) => this.getField(field) !== "");
    }
    hasChanged() {
      return Object.keys(this._changed).length > 0 || this._tagsChanged || JSON.stringify(this._creators) !== this._savedCreators;
    }
    getTags() {
      return this._tags.map((tag) => ({ tag, type: 0 }));
    }
    hasTag(tag) {
      return this._tags.includes(tag);
    }
    addTag(tag) {
      this._assertWritable("tags");
      if (this._tags.includes(tag)) return false;
      this._tags.push(tag);
      this._tagsChanged = true;
      return true;
    }
    removeTag(tag) {
      this._assertWritable("tags");
      const before = this._tags.length;
      this._tags = this._tags.filter((t) => t !== tag);
      const removed = this._tags.length !== before;
      this._tagsChanged ||= removed;
      return removed;
    }
    setTags(tags) {
      this._assertWritable("tags");
      if (!Array.isArray(tags)) throw new Error("Invalid field tags");
      const next = [...new Set(tags.map((entry) => typeof entry === "string" ? entry : entry.tag))];
      if (JSON.stringify(next) === JSON.stringify(this._tags)) return false;
      this._tags = next;
      this._tagsChanged = true;
      return true;
    }
    eraseTx() {
      throw new Error(this.isAnnotation() ? "Annotation field deletion is not supported" : "Plugin item deletion is not supported");
    }
    erase() { return this.eraseTx(); }
    isRegularItem() {
      return !this.isAttachment() && !this.isAnnotation() && !this.isNote();
    }
    isAttachment() {
      return this.itemType === "attachment";
    }
    isNote() {
      return this.itemType === "note";
    }
    /** A note's HTML, as Zotero keeps it: a schema-versioned div. */
    getNote() {
      return this.isNote() ? String(this._data.note || "") : "";
    }
    setNote(html) {
      this._assertWritable("note");
      html = String(html ?? "");
      if (html === this._data.note) return false;
      this._data.note = html;
      this._data.title = Zotero.Notes.noteToTitle(html);
      this._noteChanged = true;
      return true;
    }
    /** A note goes to the vault: a new one is made, a plugin's own changed. */
    async _saveNote() {
      if (Object.keys(this._changed).length || this._tagsChanged) this._assertWritable("tags");
      if (!this._noteChanged) return false;
      if (this._data._new) {
        const key = await call("notes.create", this._data.parentItem || null, this._data.note || "");
        const id = itemIDs.get(key) ?? nextItemID++;
        itemIDs.set(key, id);
        const installed = byKey.get(key);
        this.id = id;
        this.key = key;
        this._data = { ...(installed?._savedFields || this._data), paperloomPluginNote: true };
        delete this._data._new;
        byKey.set(key, this);
        byId.set(id, this);
      } else {
        await call("notes.update", this.key, this._data.note || "");
      }
      this._noteChanged = false;
      return true;
    }
    getNoteTitle() {
      return this.isNote() ? String(this._data.title || "") : "";
    }
    isAnnotation() {
      return this.itemType === "annotation";
    }
    isTopLevelItem() {
      return !this.parentItemKey;
    }
    get parentItemKey() { return this._data.parentItem || false; }
    set parentItemKey(value) { this._assertWritable("parentItemKey"); this._data.parentItem = value || undefined; }
    get parentKey() { return this.parentItemKey; }
    set parentKey(value) { this._assertWritable("parentKey"); this._data.parentItem = value || undefined; }
    get parentItemID() { return itemIDs.get(this.parentItemKey) || false; }
    set parentItemID(value) { this._assertWritable("parentItemID"); this._data.parentItem = byId.get(value)?.key; }
    get parentID() { return this.parentItemID; }
    set parentID(value) { this._assertWritable("parentID"); this._data.parentItem = byId.get(value)?.key; }
    get parentItem() { return byId.get(this.parentItemID) || false; }
    set parentItem(value) { this._assertWritable("parentItem"); this._data.parentItem = value?.key; }
    get dateAdded() { return this.getField("dateAdded"); }
    set dateAdded(_value) { this._assertWritable("dateAdded"); }
    get dateModified() { return this.getField("dateModified"); }
    set dateModified(_value) { this._assertWritable("dateModified"); }
    get attachmentContentType() { return this._data.contentType || ""; }
    isPDFAttachment() { return this.isAttachment() && this.attachmentContentType === "application/pdf"; }
    getAttachments() {
      return (this._data.attachments || []).map((data) => byKey.get(data.key)?.id).filter(Boolean);
    }
    getAnnotations(includeTrashed = false) {
      if (!this.isAttachment()) return [];
      return [...byKey.values()].filter((item) => item.isAnnotation() && item.parentItemKey === this.key
        && (includeTrashed || !item.deleted)).sort((a, b) => a.annotationSortIndex.localeCompare(b.annotationSortIndex)
          || a.key.localeCompare(b.key));
    }
    numAnnotations(includeTrashed = false) { return this.getAnnotations(includeTrashed).length; }
    getNotes(includeTrashed = false) {
      if (this.isNote() || this.isAnnotation()) return [];
      return [...byKey.values()].filter((item) => item.isNote() && item.parentItemKey === this.key
        && (includeTrashed || !item.deleted)).map((item) => item.id);
    }
    getCollections() {
      return [];
    }
    toJSON() {
      return { ...this._data, creators: this.getCreatorsJSON(), tags: this.getTags() };
    }
    saveTx() {
      const saved = this._saveTail ? this._saveTail.then(() => this._save()) : this._save();
      const tail = saved.catch(() => {});
      this._saveTail = tail;
      void tail.then(() => { if (this._saveTail === tail) this._saveTail = null; });
      return saved;
    }
    async _save() {
      if (this.isNote()) return this._saveNote();
      if (this._data._new) this._assertWritable("itemType");
      const changed = Object.keys(this._changed).length > 0;
      const savedCreators = JSON.stringify(this._creators);
      const creatorsChanged = savedCreators !== this._savedCreators;
      if (!changed && !this._tagsChanged && !creatorsChanged) return false;
      const fields = { ...this._changed };
      const tags = this._tagsChanged ? this._tags.slice() : null;
      if (creatorsChanged) fields.creators = this.getCreatorsJSON();
      this._savingFields = fields;
      try {
        await call("items.update", this.key, JSON.stringify(fields), tags);
      } finally {
        this._savingFields = null;
      }
      Object.assign(this._savedFields, fields);
      for (const [field, value] of Object.entries(fields)) {
        if (this._changed[field] === value) delete this._changed[field];
      }
      this._savedCreators = savedCreators;
      if (tags && JSON.stringify(tags) === JSON.stringify(this._tags)) this._tagsChanged = false;
      return true;
    }
    save() {
      return this.saveTx();
    }
  }

  const annotationFields = ["Type", "Text", "Comment", "Color", "Position", "PageLabel", "SortIndex"];
  for (const field of annotationFields) Object.defineProperty(Item.prototype, `annotation${field}`, {
    get() { return this.getField(`annotation${field}`); },
    set(value) { this.setField(`annotation${field}`, value); },
  });

  function setItems(list) {
    const records = new Map();
    for (const data of list) {
      if (!data || typeof data.key !== "string" || records.has(data.key)) throw new Error("Invalid Zotero item projection");
      records.set(data.key, data);
    }
    // Flat records are canonical. Older hosts still supply nested descriptors.
    for (const data of list) for (const attachment of data.attachments || []) {
      if (!records.has(attachment.key)) records.set(attachment.key, {
        ...attachment, itemType: "attachment", parentItem: data.key, deleted: !!data.deleted,
      });
    }
    list = [...records.values()];
    for (const data of list) if (data.itemType === "annotation") {
      if (records.get(data.parentItem)?.itemType !== "attachment"
        || !["highlight", "underline", "note", "ink", "text"].includes(data.annotationType)) {
        throw new Error("Invalid Zotero annotation projection");
      }
    }
    const previous = byKey;
    byKey = new Map();
    const keys = new Set(list.map((data) => data.key));
    const removed = [...previous.values()].filter((item) => !keys.has(item.key));
    list.forEach((data) => {
      let item = previous.get(data.key);
      if (item) item._install(data);
      else {
        const id = itemIDs.get(data.key) ?? nextItemID++;
        itemIDs.set(data.key, id);
        item = new Item(data, id);
      }
      byKey.set(item.key, item);
      byId.set(item.id, item);
    });
    return removed;
  }

  const observers = new Map();
  let nextObserver = 1;
  function notify(event, type, ids, extraData = {}) {
    const pending = [];
    const ordered = [...observers.entries()].sort((a, b) => a[1].priority - b[1].priority);
    for (const [id, { observer, types }] of ordered) {
      if (!observers.has(id)) continue;
      if (types && !types.includes(type)) continue;
      try {
        pending.push(Promise.resolve(observer.notify(event, type, ids, extraData)).catch(report));
      } catch (error) {
        report(error);
      }
    }
    return Promise.all(pending);
  }

  const sections = new Map();
  const columns = new Map();
  const readerTypes = new Set([
    "renderTextSelectionPopup", "renderSidebarAnnotationHeader", "renderToolbar",
    "createAnnotationContextMenu", "createViewContextMenu",
  ]);
  const readerListeners = new Map();
  const readerMenus = new Map();
  const readerViews = new Map();
  const itemMenus = new Map();
  const menuRegistrations = new Map();
  let nextMenuRegistration = 1;
  const readerEventsChanged = () => send({ type: "reader-events", types: [...readerListeners.keys()] });
  const Zotero_Tabs = {
    _tabs: [{ id: "zotero-pane", type: "library", data: {} }],
    selectedID: "zotero-pane",
    get selectedType() { return this.selectedID === "zotero-pane" ? "library" : "reader"; },
    select(id) { return call("tabs.select", id); },
  };
  function installTabs(message, emit = true) {
    if (message.seq !== undefined && message.seq <= tabsSeq) return;
    if (message.seq !== undefined) tabsSeq = message.seq;
    const previous = Zotero_Tabs._tabs;
    const readers = (message.readers || []).flatMap(({ viewID, attachmentKey, parentItemKey }) => {
      const item = byKey.get(attachmentKey);
      if (!item?.isAttachment() || item.parentItemKey !== parentItemKey) return [];
      const old = Zotero.Reader._readers.find((reader) => reader.tabID === viewID);
      return [Object.assign(old || newReader(), {
        itemID: item.id, _item: item, type: "pdf", tabID: viewID, _instanceID: viewID,
      })];
    });
    Zotero.Reader._readers = readers;
    Zotero_Tabs._tabs = [{ id: "zotero-pane", type: "library", data: {} }, ...readers.map((reader) => ({
      id: reader.tabID, type: "reader", data: { itemID: reader.itemID },
    }))];
    Zotero_Tabs.selectedID = readers.some((reader) => reader.tabID === message.selectedViewID) ? message.selectedViewID : "zotero-pane";
    if (!emit) return;
    const added = Zotero_Tabs._tabs.filter((tab) => tab.type === "reader" && !previous.some((old) => old.id === tab.id)).map((tab) => tab.id);
    const closed = previous.filter((tab) => tab.type === "reader" && !Zotero_Tabs._tabs.some((next) => next.id === tab.id)).map((tab) => tab.id);
    if (closed.length) void notify("close", "tab", closed);
    if (added.length) void notify("add", "tab", added);
  }
  /** A reader as zotero-plugin-toolkit's waitForReader sees it: already
   * initialised. The PDF itself is drawn by the app, not in this frame. */
  function newReader() {
    const ready = Promise.resolve();
    const view = {
      initializedPromise: ready,
      _selectedAnnotationIDs: [],
      _iframeWindow: { PDFViewerApplication: { initializedPromise: ready } },
    };
    return { _initPromise: ready, _lastView: view, _primaryView: view, _internalReader: { _lastView: view, _primaryView: view } };
  }

  const readerFor = (message) => Zotero.Reader._readers.find((reader) => reader._item.key === message.attachmentKey) || {
    itemID: byKey.get(message.attachmentKey)?.id || false,
    _item: byKey.get(message.attachmentKey) || false,
    type: "pdf", tabID: null, _instanceID: null,
  };
  let prefPane = null;

  /** Zotero's corner popup, shown as one app notice with its lines. */
  const progressWindows = new Set();
  class ProgressWindow {
    constructor() {
      this.headline = "";
      this.lines = [];
      const lines = this.lines;
      this.ItemProgress = class {
        constructor(icon, text) {
          this._itemText = document.createElement("span");
          this._image = document.createElement("span");
          this.setIcon(icon);
          this.setText(text);
          lines.push(this);
        }
        setText(text) {
          this.text = String(text ?? "");
          this._itemText.textContent = this.text;
        }
        setProgress() {}
        setIcon(icon) {
          this._image.setAttribute("data-item-type", String(icon ?? ""));
        }
        setError() {}
      };
    }
    changeHeadline(text) {
      this.headline = String(text ?? "");
    }
    addDescription(text) {
      this.lines.push({ text: String(text ?? "") });
    }
    addLines(texts) {
      for (const text of [].concat(texts || [])) this.lines.push({ text: String(text) });
    }
    show() {
      const text = [this.headline, ...this.lines.map((line) => line.text)].filter(Boolean).join("\n");
      if (text) {
        progressWindows.add(this);
        send({ type: "notice", text });
      }
      return true;
    }
    // Toolkit plugins update their lines after show(), then close the popup.
    // Publish that final text as well as the initial progress notice.
    startCloseTimer() { this.show(); this.close(); }
    close() { progressWindows.delete(this); }
  }

  // A child iframe would acquire another opaque sandbox origin. Emulate the
  // toolkit's window/document pair over nodes in this document instead.
  const dialogClosers = new Set();
  window.openDialog = (url, name, _features, ...args) => {
    if (url !== "about:blank") throw new Error(`Dialog ${url} is not available in the plugin runtime`);
    const previous = showing;
    const focus = document.activeElement;
    const layer = document.createElement("div");
    layer.className = "plugin-dialog-layer";
    layer.setAttribute("role", "dialog");
    layer.setAttribute("aria-modal", "true");
    layer.setAttribute("aria-label", name || "Plugin dialog");
    const head = document.createElement("div");
    head.hidden = true;
    const body = document.createElement("div");
    body.className = "plugin-dialog-content preference-pane";
    body.tabIndex = -1;
    const close = document.createElement("button");
    close.className = "plugin-dialog-close";
    close.textContent = "Close";
    layer.append(head, close, body);
    document.body.append(layer);
    // EventTarget itself is the window: native listeners get the same `this`
    // as Zotero window listeners, including the toolkit's `this.window` use.
    const win = new EventTarget();
    const doc = Object.assign(new EventTarget(), {
      head, body, documentElement: layer, defaultView: win, readyState: "complete",
      createElement: document.createElement.bind(document),
      createElementNS: document.createElementNS.bind(document),
      createTextNode: document.createTextNode.bind(document),
      createDocumentFragment: document.createDocumentFragment.bind(document),
      createXULElement: document.createXULElement.bind(document),
      querySelector: layer.querySelector.bind(layer),
      querySelectorAll: layer.querySelectorAll.bind(layer),
      getElementById: (id) => [...layer.querySelectorAll("[id]")].find((element) => element.id === String(id)) || null,
      getElementsByTagName: layer.getElementsByTagName.bind(layer),
      l10n: document.l10n,
      insertFTLIfNeeded: (...values) => window.MozXULElement.insertFTLIfNeeded(...values),
    });
    let title = "";
    Object.defineProperties(doc, {
      title: { get: () => head.querySelector("title")?.textContent || title, set: (value) => { title = String(value); layer.setAttribute("aria-label", title); } },
      activeElement: { get: () => layer.contains(document.activeElement) ? document.activeElement : body },
    });
    Object.assign(win, {
      document: doc, arguments: args, opener: window, name: name || "", closed: false,
      window: win, self: win,
      Event: window.Event, KeyboardEvent: window.KeyboardEvent,
      MozXULElement: window.MozXULElement,
      setTimeout: window.setTimeout.bind(window), clearTimeout: window.clearTimeout.bind(window),
      requestAnimationFrame: window.requestAnimationFrame.bind(window), cancelAnimationFrame: window.cancelAnimationFrame.bind(window),
      getComputedStyle: window.getComputedStyle.bind(window),
      sizeToContent() {},
      focus: () => (body.querySelector("input, textarea, select, button, [tabindex]") || body).focus(),
      alert: window.alert, confirm: window.confirm,
    });
    Object.defineProperties(win, {
      innerWidth: { get: () => body.clientWidth || window.innerWidth },
      innerHeight: { get: () => body.clientHeight || window.innerHeight },
    });
    win.close = () => {
      if (win.closed) return;
      win.closed = true;
      doc.dispatchEvent(new Event("beforeunload"));
      win.dispatchEvent(new Event("beforeunload"));
      doc.dispatchEvent(new Event("unload"));
      win.dispatchEvent(new Event("unload"));
      for (const arg of args) arg?.unloadLock?.resolve?.();
      dialogClosers.delete(win.close);
      layer.remove();
      if (previous?.isConnected) show(previous, previous === preferenceSession?.root ? "settings" : "dialog");
      else { showing = null; send({ type: "hide" }); }
      focus?.focus?.();
    };
    dialogClosers.add(win.close);
    close.addEventListener("click", () => win.close());
    // Element keyboard events normally bubble through document to window.
    // Forward copies to the emulated targets, preserving cancellation.
    for (const type of ["keydown", "keyup"]) layer.addEventListener(type, (event) => {
      for (const target of [doc, win]) {
        const forwarded = new KeyboardEvent(type, {
          key: event.key, code: event.code, ctrlKey: event.ctrlKey, metaKey: event.metaKey,
          shiftKey: event.shiftKey, altKey: event.altKey, repeat: event.repeat, cancelable: true,
        });
        target.dispatchEvent(forwarded);
        if (forwarded.defaultPrevented) event.preventDefault();
      }
      if (type === "keydown" && event.key === "Escape" && !event.defaultPrevented) win.close();
      event.stopPropagation();
    });
    show(layer, previous === preferenceSession?.root ? "settings" : "dialog");
    // Return before toolkit listeners attach, even with readyState complete.
    setTimeout(() => {
      if (win.closed) return;
      doc.dispatchEvent(new Event("DOMContentLoaded"));
      win.dispatchEvent(new Event("DOMContentLoaded"));
      win.dispatchEvent(new Event("load"));
      for (const arg of args) arg?.loadLock?.resolve?.();
    }, 0);
    return win;
  };
  window.alert = (text) => send({ type: "notice", text: String(text) });

  // confirm() is synchronous, but an in-frame prompt is asynchronous. Cancel
  // the original command, then replay that same command on explicit approval.
  // Approval exists only during the replay and only for the exact prompt.
  let confirmOrigin = null;
  let confirmApproval = null;
  let dismissConfirm = null;
  function captureConfirmOrigin(body) {
    for (const type of ["command", "keydown", "click"]) body.addEventListener(type, (event) => {
      confirmOrigin = event;
      Promise.resolve().then(() => { if (confirmOrigin === event) confirmOrigin = null; });
    }, true);
  }
  captureConfirmOrigin(document.body);
  window.confirm = (text) => {
    text = String(text);
    if (confirmApproval === text) { confirmApproval = null; return true; }
    const origin = confirmOrigin;
    if (!origin?.target || !["command", "keydown", "click"].includes(origin.type)) {
      send({ type: "notice", text: `Confirmation cancelled: ${text}` });
      return false;
    }
    if (document.querySelector(".plugin-confirm-layer")) return false;
    const layer = document.createElement("div");
    layer.className = "plugin-confirm-layer";
    layer.setAttribute("role", "alertdialog");
    layer.setAttribute("aria-modal", "true");
    const panel = document.createElement("div");
    const message = document.createElement("p");
    message.textContent = text;
    const cancel = document.createElement("button");
    cancel.textContent = "Cancel";
    const accept = document.createElement("button");
    accept.textContent = "Confirm";
    panel.append(message, cancel, accept);
    layer.append(panel);
    document.body.append(layer);
    const blocked = [...document.body.children].filter((child) => child !== layer).map((child) => [child, child.inert]);
    for (const [child] of blocked) child.inert = true;
    const dismiss = () => {
      layer.remove();
      for (const [child, inert] of blocked) child.inert = inert;
      dismissConfirm = null;
      origin.target.focus?.();
    };
    dismissConfirm = dismiss;
    cancel.addEventListener("click", dismiss);
    layer.addEventListener("keydown", (event) => {
      event.stopPropagation();
      if (event.key === "Escape") dismiss();
      if (event.key === "Tab") { event.preventDefault(); (document.activeElement === cancel ? accept : cancel).focus(); }
    });
    accept.addEventListener("click", () => {
      dismiss();
      if (!origin.target.isConnected) return;
      confirmApproval = text;
      try {
        const replay = origin.type === "keydown" ? new KeyboardEvent("keydown", {
          key: origin.key, ctrlKey: origin.ctrlKey, metaKey: origin.metaKey,
          shiftKey: origin.shiftKey, altKey: origin.altKey, bubbles: true, cancelable: true,
        }) : new Event(origin.type, { bubbles: true, cancelable: true });
        origin.target.dispatchEvent(replay);
      } finally { confirmApproval = null; }
    });
    cancel.focus();
    return false;
  };

  /** Fluent lookups over the plugin's English strings. */
  const interpolate = (value, args, seen = new Set()) => value == null ? value : value
    // English Fluent selectors used by the action-deletion confirmation.
    .replace(/\{\s*\$([\w-]+)\s*->([\s\S]*?)\n\s*\}/g, (match, name, body) => {
      if (!args || !owns(args, name)) return match;
      const branches = [...body.matchAll(/(\*?)\[([^\]]+)\]\s*([\s\S]*?)(?=\n\s*\*?\[|$)/g)];
      const category = Number(args[name]) === 1 ? "one" : "other";
      const branch = branches.find((part) => part[2] === String(args[name]))
        || branches.find((part) => part[2] === category) || branches.find((part) => part[1] === "*");
      return branch ? branch[3].trim() : match;
    })
    .replace(/\{\s*([\w-]+(?:\.[\w-]+)?)\s*\}/g, (match, id) => {
      if (!strings.has(id) || seen.has(id)) return match;
      return interpolate(strings.get(id), args, new Set([...seen, id]));
    })
    .replace(/\{\s*\$([\w-]+)\s*\}/g, (match, name) => args && owns(args, name) ? String(args[name]) : match);
  class Localization {
    formatMessagesSync(keys) {
      return (keys || []).map((key) => {
        const id = typeof key === "string" ? key : key.id;
        const args = typeof key === "string" ? undefined : key.args;
        const attributes = [];
        for (const [name, value] of strings) {
          if (!name.startsWith(`${id}.`)) continue;
          const attribute = name.slice(id.length + 1);
          const translated = interpolate(value, args);
          attributes.push({ name: attribute, value: translated });
          Object.defineProperty(attributes, attribute, { value: translated, enumerable: true, configurable: true });
        }
        const value = strings.get(id);
        return value === undefined && !attributes.length ? null : { value: interpolate(value, args) ?? null, attributes };
      });
    }
    formatValueSync(id, args) {
      return interpolate(strings.get(id), args) ?? null;
    }
    formatValuesSync(keys) {
      return (keys || []).map((key) => typeof key === "string" ? this.formatValueSync(key) : this.formatValueSync(key.id, key.args));
    }
    async formatMessages(keys) {
      return this.formatMessagesSync(keys);
    }
    async formatValue(id, args) {
      return this.formatValueSync(id, args);
    }
    async formatValues(keys) {
      return this.formatValuesSync(keys);
    }
  }

  class ZoteroPromise extends Promise {
    static delay(ms) {
      return new this((resolve) => setTimeout(resolve, ms));
    }
    static defer() {
      let resolve, reject;
      const promise = new this((yes, no) => { resolve = yes; reject = no; });
      return { promise, resolve, reject };
    }
  }

  const noOpenURLContext = "paperloom_openurl=unavailable";
  const Zotero = {
    version: "7.0",
    platformMajorVersion: 128,
    isMac: /Mac/.test(navigator.userAgent),
    isWin: /Windows/.test(navigator.userAgent),
    isLinux: /Linux/.test(navigator.userAgent),
    locale: "en-US",
    initializationPromise: Promise.resolve(),
    uiReadyPromise: Promise.resolve(),
    unlockPromise: Promise.resolve(),
    Promise: ZoteroPromise,
    Debug: { enabled: false, getConsoleViewerOutput: () => [] },
    debug: (message) => console.debug(String(message)),
    log: (message) => console.log(String(message)),
    logError: (error) => report(error),
    getMainWindow: () => window,
    getMainWindows: () => [window],
    getActiveZoteroPane: () => pane,
    Prefs,
    Item,
    ItemFields,
    ItemTypes,
    CreatorTypes,
    Date: ZoteroDate,
    // This frame has no duplicate index or OpenURL resolver. Give lint rules
    // empty results; the marker is answered locally by HTTP.request below.
    Duplicates: class {
      async getSearchObject() { return { search: async () => [] }; }
    },
    OpenURL: { createContextObject: () => noOpenURLContext },
    UndoHistory: {
      // Bookkeeping only: Paperloom has no plugin undo transaction API.
      actions: [],
      stageAction(name, args) { this.actions.push({ name, args }); },
    },
    // No feed items are supplied by the host. Plugins still use the class
    // to exclude them with instanceof when handling regular item additions.
    FeedItem: class extends Item {},
    ftl: { addResourceIds: () => {}, removeResourceIds: () => {} },
    Libraries: {
      userLibraryID: 1,
      get: () => ({ libraryID: 1, name: "My Library", libraryType: "user" }),
      getAll: () => [Zotero.Libraries.get(1)],
    },
    // A local namespace for plugin-generated rule IDs; no account is attached.
    Users: { getLocalUserKey: () => "paperloom" },
    UIProperties: { registerRoot: () => {}, unregisterRoot: () => {} },
    Items: {
      getAll: async () => [...byKey.values()].filter((item) => !item.deleted),
      getTopLevel: (items) => [...new Set(items.map((item) => {
        const visited = new Set();
        while (item?.parentItemKey) {
          if (visited.has(item.key)) return false;
          visited.add(item.key);
          item = item.parentItem;
        }
        return item || false;
      }).filter(Boolean))],
      getAsync: async (ids) => (Array.isArray(ids) ? ids.map((id) => byId.get(id)).filter(Boolean) : byId.get(ids) || false),
      get: (ids) => (Array.isArray(ids) ? ids.map((id) => byId.get(id)).filter(Boolean) : byId.get(ids) || false),
      getByLibraryAndKey: (_library, key) => byKey.get(key) || byId.get(itemIDs.get(key)) || false,
      getByLibraryAndKeyAsync: async (_library, key) => byKey.get(key) || byId.get(itemIDs.get(key)) || false,
    },
    // Paperloom has no Zotero note editor, so no editor instances exist.
    Notes: {
      _editorInstances: [],
      registerEditorInstance() {},
      unregisterEditorInstance() {},
      noteToTitle: (html) => String(html ?? "").replace(/<[^>]*>/g, "\n").split("\n").map((line) => line.trim()).find(Boolean) || "",
    },
    Annotations: {
      async toJSON(item) {
        if (!item?.isAnnotation()) throw new Error("Expected an annotation item");
        return {
          key: item.key, type: item.annotationType, text: item.annotationText,
          comment: item.annotationComment, color: item.annotationColor,
          position: JSON.parse(item.annotationPosition), sortIndex: item.annotationSortIndex,
          pageLabel: item.annotationPageLabel, tags: item.getTags(), dateModified: item.dateModified,
        };
      },
    },
    Collections: { getByLibrary: () => [], get: () => false },
    // Changes are saved one item at a time, so a transaction just runs.
    DB: { executeTransaction: async (work) => work() },
    Plugins: { addObserver: () => {}, removeObserver: () => {} },
    ProgressWindow,
    ProgressWindowSet: { closeAll: () => { for (const popup of progressWindows) popup.close(); } },
    Tags: {
      getAll: async () => [...new Set([...byKey.values()].flatMap((item) => item._tags))].map((tag) => ({ tag })),
    },
    Notifier: {
      registerObserver(observer, types, _name, priority = 100) {
        const id = `observer-${nextObserver++}`;
        observers.set(id, { observer, types, priority: Number.isFinite(priority) ? priority : 100 });
        return id;
      },
      unregisterObserver(id) {
        observers.delete(id);
      },
    },
    ItemPaneManager: {
      registerSection(options) {
        sections.set(options.paneID, options);
        const header = options.header || {};
        send({ type: "section", paneID: options.paneID, label: label(header.l10nID, header.label || options.paneID) });
        return options.paneID;
      },
      unregisterSection(paneID) {
        sections.delete(paneID);
        return true;
      },
    },
    MenuManager: {
      registerMenu(options) {
        const id = `menu-${nextMenuRegistration++}`;
        menuRegistrations.set(id, options);
        return id;
      },
      unregisterMenu: (id) => menuRegistrations.delete(id),
    },
    ItemTreeManager: {
      registerColumn(options) {
        if (!options || typeof options !== "object" || Array.isArray(options)) return false;
        if (["dataKey", "label"].some((name) => typeof options[name] !== "string" || !options[name])) return false;
        // Zotero checks only the type of pluginID; some plugins leave it empty.
        if (typeof options.pluginID !== "string") return false;
        if (["dataProvider", "renderCell"].some((name) => options[name] !== undefined && typeof options[name] !== "function")) return false;
        const key = [options.pluginID, options.dataKey].filter(Boolean).join("-").replace(/[^A-Za-z0-9-]/g, "-");
        if (columns.has(key)) return false;
        columns.set(key, { dataKey: options.dataKey, column: { ...options, dataKey: key } });
        send({ type: "column", key, label: options.label });
        return key;
      },
      registerColumns(options) {
        return Array.isArray(options)
          ? options.map((option) => Zotero.ItemTreeManager.registerColumn(option))
          : Zotero.ItemTreeManager.registerColumn(options);
      },
      unregisterColumn(key) {
        if (!columns.delete(key)) return false;
        send({ type: "column-removed", key });
        return true;
      },
      unregisterColumns(keys) {
        return Array.isArray(keys)
          ? keys.map((key) => Zotero.ItemTreeManager.unregisterColumn(key))
          : Zotero.ItemTreeManager.unregisterColumn(keys);
      },
    },
    Reader: {
      _readers: [],
      getByTabID(id) { return this._readers.find((reader) => reader.tabID === id) || null; },
      registerEventListener(type, handler, pluginID) {
        if (!readerTypes.has(type)) return partial("reader extensions");
        if (typeof handler !== "function") return;
        const added = !readerListeners.has(type);
        if (added) readerListeners.set(type, new Map());
        readerListeners.get(type).set(handler, pluginID);
        if (added) readerEventsChanged();
      },
      unregisterEventListener(type, handler) {
        if (!readerTypes.has(type)) return partial("reader extensions");
        const listeners = readerListeners.get(type);
        if (!listeners || !listeners.delete(handler) || listeners.size) return;
        readerListeners.delete(type);
        readerEventsChanged();
      },
    },
    PreferencePanes: {
      register(options) {
        prefPane = options;
        send({ type: "settings-available" });
        return Promise.resolve(options.id || options.pluginID);
      },
      unregister(id) {
        if (prefPane && [prefPane.id, prefPane.pluginID].includes(id)) {
          closePreferences();
          prefPane = null;
        }
      },
    },
    HTTP: {
      async request(method, url, options = {}) {
        if (method === "GET" && String(url).startsWith("https://www.crossref.org/openurl?")
          && String(url).includes(`&${noOpenURLContext}&`)) {
          const responseText = '<crossref_result><query status="unresolved"/></crossref_result>';
          const responseXML = { getElementsByTagName: (name) => name === "query"
            ? [{ getAttribute: (attribute) => attribute === "status" ? "unresolved" : null, getElementsByTagName: () => [] }]
            : [] };
          return { status: 200, responseText, response: responseXML, responseXML };
        }
        // Bundled icons and data already belong to this plugin's sandbox.
        // Keep these reads local; only web requests go through the host.
        const packaged = String(url).startsWith(ROOT) || /^chrome:\/\/[^/]+\/content\//.test(String(url));
        if (packaged && method !== "GET") throw new Error("Packaged resources only support GET");
        const answer = packaged ? { status: 200, text: fileText(url) } : await call("requestUrl", {
          url,
          method,
          headers: options.headers || {},
          body: typeof options.body === "string" ? options.body : options.body ? JSON.stringify(options.body) : null,
        });
        const ok = options.successCodes === false || (answer.status >= 200 && answer.status < 300);
        if (!ok) throw new Error(`HTTP ${answer.status} for ${url}`);
        let response = answer.text;
        if (options.responseType === "json") {
          try {
            response = JSON.parse(answer.text);
          } catch {
            response = null;
          }
        }
        return { status: answer.status, responseText: answer.text, response };
      },
    },
    File: {
      getContentsAsync: async (path) => fileText(path),
      getContents: (path) => fileText(path),
    },
    Utilities: {
      sentenceCase,
      capitalizeName,
      capitalizeTitle,
      throttle(fn, wait) {
        let lastCall, timer, pendingArgs, pendingThis, result;
        const invoke = () => {
          lastCall = Date.now();
          timer = undefined;
          const args = pendingArgs, receiver = pendingThis;
          pendingArgs = pendingThis = undefined;
          result = fn.apply(receiver, args);
        };
        return function (...args) {
          pendingArgs = args;
          pendingThis = this;
          const remaining = lastCall === undefined ? 0 : wait - (Date.now() - lastCall);
          if (remaining <= 0) {
            if (timer !== undefined) clearTimeout(timer);
            invoke();
          } else if (timer === undefined) timer = setTimeout(invoke, remaining);
          return result;
        };
      },
      trimInternal: (text) => String(text).replace(/\s+/g, " ").trim(),
      cleanDOI: (text) => {
        text = String(text ?? "");
        if (/^https?:/i.test(text.trim())) {
          try { text = decodeURIComponent(text); } catch { /* Leave invalid URL escapes as supplied. */ }
        }
        const match = /10\.\d{4,9}\/[^\s<>"]+/.exec(text);
        if (!match) return null;
        let doi = match[0].replace(/[.,]+$/, "");
        const brackets = { ")": "(", "]": "[", "}": "{" };
        const close = doi.slice(-1), open = brackets[close];
        const prefix = text.slice(0, match.index);
        if (open && prefix.lastIndexOf(open) > prefix.lastIndexOf(close)) doi = doi.slice(0, -1);
        return doi;
      },
      cleanISBN,
      isEmpty: (object) => !object || Object.keys(object).length === 0,
      randomString: (length = 8) => Math.random().toString(36).slice(2, 2 + length),
      Internal: { md5: () => "", itemToExportFormat: (item) => (item && item.toJSON ? item.toJSON() : item) },
    },
    getString: (key) => label(key),
  };

  /** Mozilla's version order, enough for plain dotted versions. */
  function compareVersions(a, b) {
    const parts = (version) => String(version ?? "").split(".").map((part) => parseInt(part, 10) || 0);
    const [x, y] = [parts(a), parts(b)];
    for (let i = 0; i < Math.max(x.length, y.length); i++) {
      if ((x[i] || 0) !== (y[i] || 0)) return (x[i] || 0) < (y[i] || 0) ? -1 : 1;
    }
    return 0;
  }

  function loadSubScript(url, scope) {
    const target = scope || window;
    // Pane scripts use script-global var/function declarations across files.
    if (target === window) {
      (0, eval)(fileText(url));
      return;
    }
    // The script sees its scope's names and sets them with `this`.
    new Function("__scope", `with (__scope) {\n${fileText(url)}\n}`).call(target, target);
  }

  function prefBranch(prefix = "", defaultsOnly = false) {
    const get = (key, fallback, convert) => {
      key = prefix + key;
      const value = defaultsOnly ? defaultPrefs[key] : prefValue(key);
      return value === undefined ? fallback : convert(value);
    };
    const set = (key, value) => {
      key = prefix + key;
      if (defaultsOnly) {
        defaultPrefs[key] = value;
        if (!owns(prefs, key)) prefChanged(key);
      } else Prefs.set(key, value, true);
    };
    return {
      getCharPref: (key, fallback) => get(key, fallback, String),
      setCharPref: (key, value) => set(key, String(value)),
      getBoolPref: (key, fallback) => get(key, fallback, Boolean),
      setBoolPref: (key, value) => set(key, Boolean(value)),
      getIntPref: (key, fallback) => get(key, fallback, Number),
      setIntPref: (key, value) => set(key, Number(value)),
      clearUserPref: (key) => Prefs.clear(prefix + key, true),
      getBranch: (name = "") => prefBranch(prefix + name, defaultsOnly),
      getDefaultBranch: (name = "") => prefBranch(prefix + name, true),
      getChildList: (name = "") => [...new Set([...Object.keys(defaultPrefs), ...Object.keys(prefs)])]
        .filter((key) => key.startsWith(prefix + name)).map((key) => key.slice(prefix.length)).sort(),
    };
  }

  const Services = {
    scriptloader: { loadSubScript },
    prefs: prefBranch(),
    wm: {
      getMostRecentWindow: () => window,
      getEnumerator: () => {
        const iterator = [window][Symbol.iterator]();
        let remaining = true;
        return {
          [Symbol.iterator]() { return this; },
          next() { remaining = false; return iterator.next(); },
          hasMoreElements: () => remaining,
          getNext() { return this.next().value; },
        };
      },
      addListener: () => {},
      removeListener: () => {},
    },
    ww: { openWindow: (_parent, url, name, features, args) => window.openDialog(url, name, features, args) },
    prompt: {
      alert: (_window, title, text) => send({ type: "notice", text: [title, text].filter(Boolean).join("\n") }),
      confirm: (_window, title, text) => window.confirm([title, text].filter(Boolean).join("\n")),
    },
    vc: { compare: compareVersions },
    obs: { addObserver: () => {}, removeObserver: () => {}, notifyObservers: () => {} },
    locale: { appLocaleAsBCP47: "en-US", requestedLocale: "en-US" },
    appinfo: { OS: navigator.userAgent.includes("Mac") ? "Darwin" : "Linux" },
  };
  // The Gecko modules plugins import, as far as plugins use them here.
  const geckoModules = {
    "Console": { ConsoleAPI: function ConsoleAPI() { return console; } },
    "concurrentCaller": { ConcurrentCaller: class {
      constructor(options = {}) {
        this.options = options;
        this.limit = Math.max(1, Math.floor(Number(options.numConcurrent) || 1));
        this.queue = [];
        this.running = 0;
        this.idle = [];
      }
      start(work) {
        if (Array.isArray(work)) return Promise.all(work.map((task) => this.start(task)));
        const promise = new Promise((resolve, reject) => this.queue.push({ work, resolve, reject }));
        // Plugins such as Linter enqueue tasks without awaiting each result;
        // they receive errors through onError and await the whole batch.
        promise.catch(() => {});
        this.drain();
        return promise;
      }
      drain() {
        while (this.running < this.limit && this.queue.length) {
          const task = this.queue.shift();
          this.running++;
          Promise.resolve().then(task.work).then(task.resolve, (error) => {
            task.reject(error);
            report(error);
            try { this.options.onError?.(error); } catch (failure) { report(failure); }
            if (this.options.stopOnError) this.stop();
          }).finally(() => {
            this.running--;
            this.drain();
          });
        }
        if (!this.running && !this.queue.length) {
          for (const resolve of this.idle.splice(0)) resolve();
        }
      }
      wait() {
        return !this.running && !this.queue.length ? Promise.resolve()
          : new Promise((resolve) => this.idle.push(resolve));
      }
      stop() {
        const error = new Error("Concurrent caller stopped");
        error.name = "CanceledException";
        for (const task of this.queue.splice(0)) task.reject(error);
        this.drain();
      }
    } },
    "AddonManager": {
      AddonManager: {
        getAddonByID: async () => null,
        getAllAddons: async () => [],
        addAddonListener: () => {},
        removeAddonListener: () => {},
      },
    },
  };
  const geckoModule = (url) => geckoModules[/([A-Za-z]+)\.(?:jsm|(?:sys\.)?mjs)$/.exec(String(url))?.[1]] || {};
  const ChromeUtils = {
    import: geckoModule,
    importESModule: geckoModule,
    defineESModuleGetters: () => {},
  };
  // XPCOM as Zotero 7 bootstraps reach for it: the add-on manager registers
  // a plugin's chrome folder, and Zotero itself is a service.
  const xpcomServices = {
    "@mozilla.org/addons/addon-manager-startup;1": {
      registerChrome(_manifest, entries) {
        const content = (entries || []).filter(([kind]) => kind === "content");
        for (const [, name, path] of content) chromePackages.set(String(name), String(path));
        return { destruct: () => content.forEach(([, name]) => chromePackages.delete(String(name))) };
      },
    },
    "@zotero.org/Zotero;1": { wrappedJSObject: Zotero },
  };
  const service = (id) => xpcomServices[id] || {};
  const classes = new Proxy({}, {
    get: (_, id) => ({ getService: () => service(String(id)), createInstance: () => service(String(id)) }),
  });
  const interfaces = new Proxy({}, { get: (_, name) => String(name) });
  const Components = {
    classes, interfaces,
    utils: {
      import: () => ({}),
      // Browser globals (such as FormData) already live on this sandbox's window.
      importGlobalProperties: () => {},
      getGlobalForObject: () => window,
      isDeadWrapper: () => false,
    },
    results: {}, ID: (id) => id,
  };
  Services.io = {
    newURI: (spec) => ({ spec: String(spec), asciiSpec: String(spec) }),
    // Plugins may add zotero:// pages; the app opens none of them.
    getProtocolHandler: () => ({ wrappedJSObject: { _extensions: {} } }),
  };
  if (!document.createXULElement) document.createXULElement = (tag) => {
    const element = document.createElement(tag);
    for (const name of ["hidden", "disabled", "checked", "label"]) {
      Object.defineProperty(element, name, {
        configurable: true,
        get: () => name === "label" ? element.getAttribute(name) || "" : element.getAttribute(name) === "true",
        set: (value) => {
          if (name === "label") element.setAttribute(name, String(value));
          else if (value) element.setAttribute(name, "true");
          else element.removeAttribute(name);
        },
      });
    }
    return element;
  };
  // DOM dispatch ignores listener return values. Track command promises so
  // a temporary command selection survives until asynchronous handlers finish.
  const createXUL = document.createXULElement.bind(document);
  document.createXULElement = (tag) => {
    const element = createXUL(tag);
    const add = element.addEventListener.bind(element);
    const remove = element.removeEventListener?.bind(element);
    const wrappers = new Map();
    element.addEventListener = (type, listener, options) => {
      if (type !== "command" || !listener) return add(type, listener, options);
      if (!wrappers.has(listener)) wrappers.set(listener, function(event) {
        const result = typeof listener === "function" ? listener.call(this, event) : listener.handleEvent(event);
        if (result?.then && event._pluginPromises) event._pluginPromises.push(Promise.resolve(result).catch(report));
      });
      return add(type, wrappers.get(listener), options);
    };
    element.removeEventListener = (type, listener, options) => remove?.(type, type === "command" ? wrappers.get(listener) || listener : listener, options);
    return element;
  };
  const itemPopup = document.createXULElement("menupopup");
  itemPopup.setAttribute("id", "zotero-itemmenu");
  itemPopup.setAttribute("hidden", "true");
  document.body.append(itemPopup);
  const menuState = (element) => ({
    hidden: element.getAttribute("hidden") === "true",
    disabled: element.getAttribute("disabled") === "true",
  });
  // Check the current tree as well as the remembered element: plugins can
  // replace entries or change their parent menu between requests.
  function commandPath(element) {
    if (element?.registration) {
      const { id, registration, ancestors, menuElem } = element;
      if (menuRegistrations.get(id) !== registration) return null;
      if ([...ancestors, menuElem].some((node) => {
        const state = menuState(node);
        return state.hidden || state.disabled;
      })) return null;
      // The host accepts one submenu level; deeper menus share that level.
      return [...(ancestors.length ? [ancestors[0].label] : []), menuElem.label];
    }
    if (!element || element.localName !== "menuitem") return null;
    const path = [element.getAttribute("label")];
    for (let node = element; node !== itemPopup; node = node.parentElement) {
      if (!node) return null;
      const state = menuState(node);
      if (state.hidden || state.disabled) return null;
      if (node.localName === "menu") path.unshift(node.getAttribute("label"));
    }
    return path;
  }
  const samePath = (left, right) => left && left.length === right.length && left.every((label, index) => label === right[index]);
  function menuContext(menuElem) {
    return {
      items: pane.getSelectedItems(),
      fieldName: undefined,
      menuElem,
      setVisible: (visible) => { menuElem.hidden = !visible; },
      setEnabled: (enabled) => { menuElem.disabled = !enabled; },
      setL10nArgs: (args) => document.l10n.setAttributes(menuElem, menuElem.getAttribute("data-l10n-id"), args),
    };
  }

  function readRegisteredMenus(options) {
    const items = [];
    for (const [id, registration] of [...menuRegistrations]) {
      if (registration.target !== "main/library/item") continue;
      const read = (menus, ancestors = []) => {
        const result = [];
        for (const menu of menus || []) {
          try {
            const menuElem = document.createXULElement(menu.menuType === "submenu" ? "menu" : menu.menuType === "separator" ? "menuseparator" : "menuitem");
            const context = menuContext(menuElem);
            let localizedID, localizedArgs;
            const localize = () => {
              const l10nID = menuElem.getAttribute("data-l10n-id");
              const l10nArgs = menuElem.getAttribute("data-l10n-args");
              if (l10nID && (l10nID !== localizedID || l10nArgs !== localizedArgs)) {
                menuElem.label = interpolate(label(l10nID), l10nArgs ? JSON.parse(l10nArgs) : undefined);
              }
              localizedID = l10nID;
              localizedArgs = l10nArgs;
            };
            if (menu.l10nID) {
              menuElem.setAttribute("data-l10n-id", menu.l10nID);
              if (menu.l10nArgs !== undefined) menuElem.setAttribute("data-l10n-args", JSON.stringify(menu.l10nArgs));
              localize();
            }
            if (menu.label !== undefined) menuElem.label = menu.label;
            menu.onShowing?.(new Event("popupshowing"), context);
            localize();
            if (menuElem.hidden) continue;
            menu.onShown?.(new Event("popupshown"), context);
            localize();
            if (menuElem.hidden) continue;
            if (menu.menuType === "separator") {
              result.push({ separator: true });
            } else if (menu.menuType === "submenu") {
              const children = read(menu.menus, [...ancestors, menuElem]);
              if (!menuElem.label.trim() || !children.length) continue;
              // Flatten deeper submenus, retaining their visibility/enabled gates.
              if (ancestors.length) result.push(...children);
              else result.push({ label: menuElem.label, disabled: menuElem.disabled, children });
            } else if (menu.menuType === "menuitem" && menuElem.label.trim()) {
              result.push({ index: options.length, label: menuElem.label,
                disabled: [...ancestors, menuElem].some((element) => element.disabled) });
              options.push({ id, registration, menu, menuElem, ancestors });
            }
          } catch (error) {
            report(error);
          }
        }
        return ancestors.length ? cleanSeparators(result) : result;
      };
      items.push(...read(registration.menus));
    }
    return items;
  }

  function cleanSeparators(items) {
    const result = [];
    for (const item of items) {
      if (!item.separator || (result.length && !result[result.length - 1].separator)) result.push(item);
    }
    if (result[result.length - 1]?.separator) result.pop();
    return result;
  }

  function readItemMenu() {
    const options = [];
    const readPopup = (popup, nested = false) => {
      try {
        popup.dispatchEvent(new Event("popupshowing", { bubbles: true }));
      } catch (error) {
        report(error);
      }
      const items = [];
      for (const element of popup.children) {
        const { hidden, disabled } = menuState(element);
        if (hidden) continue;
        if (element.localName === "menuseparator") {
          if (items.length && !items[items.length - 1].separator) items.push({ separator: true });
          continue;
        }
        const label = element.getAttribute("label");
        if (!label || !label.trim()) continue;
        if (element.localName === "menuitem") {
          items.push({ index: options.length, label, disabled });
          options.push(element);
        } else if (element.localName === "menu" && !nested) {
          const submenu = element.querySelector("menupopup");
          if (submenu && !menuState(submenu).hidden) {
            const children = readPopup(submenu, true);
            if (children.length) items.push({ label, disabled: disabled || menuState(submenu).disabled, children });
          }
        }
      }
      if (items[items.length - 1]?.separator) items.pop();
      return items;
    };
    let items = [];
    itemMenuErrorReported = false;
    try {
      items = cleanSeparators([...readPopup(itemPopup), ...readRegisteredMenus(options)]);
    } catch (error) {
      report(error);
    } finally {
      itemMenuErrorReported = null;
    }
    return { items, options };
  }
  Object.assign(window, { Localization });
  class RuntimeXULElement extends HTMLElement {
    static insertFTLIfNeeded() {}
    static parseXULToFragment(markup) {
      const template = document.createElement("template");
      template.innerHTML = markup;
      return template.content;
    }
    static implementCustomInterface() {}
  }
  if (!window.MozXULElement) window.MozXULElement = RuntimeXULElement;
  if (!window.XULElementBase) window.XULElementBase = typeof window.MozXULElement === "function"
    ? window.MozXULElement : RuntimeXULElement;
  function localizeElement(element) {
    const id = element.getAttribute("data-l10n-id");
    if (!id) return;
    const args = element.getAttribute("data-l10n-args");
    const message = new Localization().formatMessagesSync([{ id, args: args ? JSON.parse(args) : undefined }])[0];
    if (!message) return;
    if (message.value !== null) {
      if (["checkbox", "radio"].includes(element.localName)) element.setAttribute("label", message.value);
      else element.textContent = message.value;
    }
    for (const { name, value } of message.attributes) {
      element.setAttribute(name, value);
      if (name === "value" && ["label", "description"].includes(element.localName)) element.textContent = value;
      if (name === "tooltiptext") {
        element.setAttribute("title", value);
        // XUL icon-only buttons have no browser icon; keep them discoverable.
        if (element.localName === "button" && !element.textContent.trim() && !element.hasAttribute("label")) element.textContent = value;
      }
    }
    syncControl(element);
  }
  if (!document.l10n) {
    document.l10n = {
      setAttributes(element, id, args) {
        element.setAttribute("data-l10n-id", id);
        if (args !== undefined) element.setAttribute("data-l10n-args", JSON.stringify(args));
        else element.removeAttribute("data-l10n-args");
        localizeElement(element);
      },
      async translateFragment(root) {
        for (const element of root.querySelectorAll("[data-l10n-id]")) localizeElement(element);
      },
      async translateElements(elements) { elements.forEach(localizeElement); },
      formatValue: (id, args) => new Localization().formatValue(id, args),
      formatValues: (keys) => new Localization().formatValues(keys),
    };
  }
  if (!window.ZoteroContextPane) window.ZoteroContextPane = { context: null };
  Object.assign(window, { Zotero, Zotero_Tabs, ZoteroPane: pane, ZoteroPane_Local: pane, Services, ChromeUtils, Components, Cc: classes, Ci: interfaces, Cu: Components.utils });
  // The reasons bootstrap functions are called with.
  Object.assign(window, {
    APP_STARTUP: 1, APP_SHUTDOWN: 2, ADDON_ENABLE: 3, ADDON_DISABLE: 4,
    ADDON_INSTALL: 5, ADDON_UNINSTALL: 6, ADDON_UPGRADE: 7, ADDON_DOWNGRADE: 8,
  });

  // Keep XUL tag names for plugin selectors while using native browser inputs.
  // Controls created by scripts and cloned template rows get the same adapter.
  const controls = new WeakMap();
  const rawCreateXUL = document.createXULElement.bind(document);
  const rawCreateNS = document.createElementNS?.bind(document);
  if (rawCreateNS) document.createElementNS = (namespace, name, ...args) =>
    rawCreateNS(namespace, namespace === "http://www.w3.org/1999/xhtml" ? name.replace(/^html:/, "") : name, ...args);
  document.createXULElement = (tag) => adaptControl(rawCreateXUL(tag));
  if (!owns(HTMLElement.prototype, "ownerGlobal")) Object.defineProperty(HTMLElement.prototype, "ownerGlobal", { get: () => window });

  function syncControl(element) {
    const control = controls.get(element);
    if (!control) return;
    const { input, caption } = control;
    if (caption) caption.textContent = element.getAttribute("label") || "";
    if (input) input.disabled = element.getAttribute("disabled") === "true";
    if (element.localName === "button" && element.hasAttribute("label")) element.textContent = element.getAttribute("label");
  }

  function adaptControl(element) {
    if (controls.has(element)) return element;
    const tag = element.localName;
    if (!["checkbox", "radio", "radiogroup", "menulist", "button"].includes(tag)) return element;
    const control = {};
    controls.set(element, control);
    const attribute = element.setAttribute.bind(element);
    const removeAttribute = element.removeAttribute.bind(element);
    element.setAttribute = (name, value) => {
      if (["disabled", "hidden", "checked"].includes(name) && String(value) === "false") removeAttribute(name);
      else attribute(name, String(value));
      if (tag === "menulist" && name === "value" && control.refresh && element.value !== String(value)) element.value = value;
      syncControl(element);
    };
    element.removeAttribute = (name) => { removeAttribute(name); syncControl(element); };
    const property = (name, get, set) => Object.defineProperty(element, name, { configurable: true, get, set });
    property("label", () => element.getAttribute("label") || "", (value) => element.setAttribute("label", value));
    if (tag === "checkbox" || tag === "radio") {
      const label = document.createElement("label");
      const input = document.createElement("input");
      const caption = document.createElement("span");
      input.type = tag === "checkbox" ? "checkbox" : "radio";
      label.append(input, caption);
      element.append(label);
      Object.assign(control, { input, caption });
      property("checked", () => input.checked, (value) => { input.checked = Boolean(value); });
      property("disabled", () => input.disabled, (value) => value ? element.setAttribute("disabled", "true") : element.removeAttribute("disabled"));
      input.checked = element.getAttribute("checked") === "true";
      input.addEventListener("change", () => {
        const group = tag === "radio" ? element.closest("radiogroup") : null;
        if (group) group.value = element.getAttribute("value");
        (group || element).dispatchEvent(new Event("command", { bubbles: true }));
      });
    } else if (tag === "radiogroup") {
      let value = "";
      property("value", () => value, (next) => {
        value = String(next);
        if (element.getAttribute("value") !== value) attribute("value", value);
        for (const radio of element.querySelectorAll("radio")) {
          adaptControl(radio);
          radio.checked = radio.getAttribute("value") === value;
          controls.get(radio).input.name = element.id || (control.name ||= `pref-radio-${nextCall++}`);
        }
      });
    } else if (tag === "menulist") {
      let value = "";
      const select = document.createElement("select");
      control.input = select;
      // The menuitem nodes remain the model; the select is their visible view.
      control.refresh = () => {
        if (!select.parentNode) element.append(select);
        select.replaceChildren();
        for (const item of element.querySelectorAll("menuitem")) {
          const option = document.createElement("option");
          option.value = item.getAttribute("value") ?? item.getAttribute("label") ?? "";
          option.textContent = item.getAttribute("label") || item.textContent;
          select.append(option);
        }
        select.value = value;
      };
      property("value", () => value, (next) => { value = String(next); if (element.getAttribute("value") !== value) attribute("value", value); control.refresh(); });
      property("selectedIndex", () => select.selectedIndex, (next) => {
        control.refresh();
        select.selectedIndex = next;
        value = select.value;
        if (element.getAttribute("value") !== value) attribute("value", value);
      });
      element.appendItem = (label, itemValue) => {
        let popup = element.querySelector("menupopup");
        if (!popup) { popup = document.createXULElement("menupopup"); element.prepend(popup); }
        const item = document.createXULElement("menuitem");
        item.setAttribute("label", label);
        item.setAttribute("value", itemValue);
        popup.append(item);
        control.refresh();
        return item;
      };
      select.addEventListener("change", () => {
        value = select.value;
        attribute("value", value);
        element.dispatchEvent(new Event("command", { bubbles: true }));
      });
      Promise.resolve().then(() => { value = element.getAttribute("value") || value; control.refresh(); });
      property("disabled", () => select.disabled, (next) => next ? element.setAttribute("disabled", "true") : element.removeAttribute("disabled"));
    } else if (tag === "button") {
      element.addEventListener("click", () => element.dispatchEvent(new Event("command", { bubbles: true })));
    }
    syncControl(element);
    return element;
  }

  // Clone plain markup, then restore adapters. Runtime-owned native controls
  // are rebuilt, so cloned rows never inherit a stale select or checkbox view.
  function clonePaneNode(node, deep) {
    const copy = document.createElement(node.localName);
    for (const { name, value } of node.attributes) copy.setAttribute(name, value);
    if (deep) for (const child of node.childNodes) {
      if (child.nodeType === 1 && ![controls.get(node)?.input, controls.get(node)?.input?.parentElement].includes(child)) copy.append(clonePaneNode(child, true));
      else if (child.nodeType === 3) copy.append(document.createTextNode(child.textContent));
    }
    preparePaneNode(copy);
    return copy;
  }
  function preparePaneNode(element) {
    adaptControl(element);
    element.cloneNode = (deep = false) => clonePaneNode(element, deep);
  }

  function resourceURL(path, base) { return new URL(path, base || ROOT).href; }
  function paneMarkup(text) {
    // XML parsing preserves self-closing XUL/HTML nodes and table structure.
    const parsed = new DOMParser().parseFromString(`<pane xmlns:html="http://www.w3.org/1999/xhtml">${text.replace(/<\?xml[^>]*\?>/g, "")}</pane>`, "application/xml");
    const error = parsed.querySelector("parsererror");
    if (error) throw new Error(`Invalid preference pane: ${error.textContent}`);
    const convert = (node) => {
      if (node.nodeType !== 1) return node.nodeType === 3 && node.textContent.trim() ? document.createTextNode(node.textContent) : null;
      const element = node.namespaceURI && node.namespaceURI !== "http://www.w3.org/1999/xhtml"
        ? document.createElementNS(node.namespaceURI, node.nodeName) : document.createElement(node.localName);
      for (const { name, value } of node.attributes) if (!name.startsWith("xmlns")) element.setAttribute(name, value);
      for (const child of node.childNodes) { const converted = convert(child); if (converted) element.append(converted); }
      if (node.localName === "script") element.setAttribute("type", "text/plain");
      preparePaneNode(element);
      return element;
    };
    return [...parsed.documentElement.childNodes].map(convert).filter(Boolean);
  }

  // Minimal descriptors support the toolkit's table mount, not arbitrary React.
  const React = { createElement: (type, props, ...children) => ({ type, props: props || {}, children }) };
  const IntlProvider = Symbol("IntlProvider");
  class VirtualizedTable {
    static makeRowRenderer(getRowData) { return (index) => getRowData(index); }
    constructor(container, props) {
      this.container = container;
      this.props = props;
      this.anchor = -1;
      const changed = () => { this.paintSelection(); props.onSelectionChange?.(this.selection); };
      this.selection = {
        selected: new Set(), focused: -1,
        select: (i) => {
          this.selection.selected.clear();
          if (i >= 0 && i < props.getRowCount()) this.selection.selected.add(i);
          this.selection.focused = this.selection.selected.size ? i : -1;
          this.anchor = this.selection.focused;
          changed();
        },
        toggleSelect: (i) => {
          if (i < 0 || i >= props.getRowCount()) return;
          if (this.selection.selected.has(i)) this.selection.selected.delete(i);
          else this.selection.selected.add(i);
          this.selection.focused = i;
          this.anchor = i;
          changed();
        },
        clearSelection: () => { this.selection.selected.clear(); this.selection.focused = -1; this.anchor = -1; changed(); },
        isSelected: (i) => this.selection.selected.has(i),
        _updateTree() {},
      };
      this.table = document.createElement("table");
      this.table.className = "plugin-virtualized-table";
      this.table.id = props.id || "";
      this.table.tabIndex = 0;
      this.table.setAttribute("aria-label", "Rules");
      this.table.setAttribute("aria-multiselectable", String(Boolean(props.multiSelect)));
      this.table.addEventListener("keydown", (event) => {
        if (props.onKeyDown?.(event) === false) { event.preventDefault(); return; }
        if (event.defaultPrevented) return;
        if (event.key === "Enter") { event.preventDefault(); props.onActivate?.(event); }
        else if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
          event.preventDefault();
          const last = props.getRowCount() - 1;
          const next = event.key === "Home" ? 0 : event.key === "End" ? last
            : Math.max(0, Math.min(last, this.selection.focused + (event.key === "ArrowDown" ? 1 : -1)));
          this.choose(next, event);
        } else if (event.key === " " && props.multiSelect) { event.preventDefault(); this.selection.toggleSelect(this.selection.focused); }
      });
      container.replaceChildren(this.table);
      this.invalidate();
      props.ref?.(this);
    }
    choose(index, event) {
      if (this.props.multiSelect && event.shiftKey && this.anchor >= 0) {
        if (!event.ctrlKey && !event.metaKey) this.selection.selected.clear();
        for (let i = Math.min(this.anchor, index); i <= Math.max(this.anchor, index); i++) this.selection.selected.add(i);
        this.selection.focused = index;
        this.paintSelection();
        this.props.onSelectionChange?.(this.selection);
      } else if (this.props.multiSelect && (event.ctrlKey || event.metaKey)) this.selection.toggleSelect(index);
      else this.selection.select(index);
    }
    paintSelection() {
      for (const row of this.table.querySelectorAll("tbody tr")) {
        const selected = this.selection.isSelected(Number(row.dataset.index));
        row.classList.toggle("selected", selected);
        row.setAttribute("aria-selected", String(selected));
      }
    }
    invalidate() {
      const count = this.props.getRowCount();
      let trimmed = false;
      for (const i of this.selection.selected) if (i >= count) { this.selection.selected.delete(i); trimmed = true; }
      if (this.selection.focused >= count) this.selection.focused = count - 1;
      if (this.anchor >= count) this.anchor = this.selection.focused;
      const columns = this.props.columns || [];
      this.table.replaceChildren();
      if (this.props.showHeader) {
        const head = document.createElement("thead");
        const row = document.createElement("tr");
        columns.forEach((column, index) => {
          const cell = document.createElement("th");
          cell.scope = "col";
          const direction = column.sortDirection;
          cell.setAttribute("aria-sort", direction ? direction > 0 ? "ascending" : "descending" : "none");
          const button = document.createElement("button");
          button.textContent = `${column.label || column.dataKey || ""}${direction ? direction > 0 ? " ▲" : " ▼" : ""}`;
          button.addEventListener("click", () => {
            const next = column.sortDirection === 1 ? -1 : 1;
            this.props.columns = columns.map((col, i) => ({ ...col, sortDirection: i === index ? next : undefined }));
            this.props.onColumnSort?.(index, next);
            this.forceUpdate();
          });
          if (column.width) cell.style.width = typeof column.width === "number" ? `${column.width}px` : column.width;
          cell.append(button);
          row.append(cell);
        });
        head.append(row);
        this.table.append(head);
      }
      const body = document.createElement("tbody");
      for (let index = 0; index < count; index++) {
        const data = this.props.renderItem ? this.props.renderItem(index) : this.props.getRowData(index);
        const row = document.createElement("tr");
        row.dataset.index = String(index);
        for (const column of columns) {
          const cell = document.createElement("td");
          cell.textContent = String(data?.[column.dataKey] ?? "");
          row.append(cell);
        }
        row.addEventListener("click", (event) => { this.table.focus(); this.choose(index, event); });
        row.addEventListener("dblclick", (event) => { this.selection.focused = index; this.props.onActivate?.(event); });
        body.append(row);
      }
      this.table.append(body);
      this.paintSelection();
      if (trimmed) this.props.onSelectionChange?.(this.selection);
    }
    forceUpdate() { this.invalidate(); }
  }
  const ReactDOM = { createRoot(container) {
    if (!container) throw new Error("Table container is missing");
    let instance;
    const render = (descriptor) => {
      if (descriptor?.type === IntlProvider) { for (const child of descriptor.children) render(child); }
      else if (descriptor?.type === VirtualizedTable) instance = new VirtualizedTable(container, descriptor.props);
      else throw new Error("Only IntlProvider and VirtualizedTable descriptors are available in the plugin runtime");
    };
    return { render, unmount: () => { instance?.props.ref?.(null); container.replaceChildren(); } };
  } };
  Zotero.Intl ||= {};
  Zotero.Intl.strings ||= {};
  if (!window.require) window.require = (name) => {
    if (name === "react") return React;
    if (name === "react-dom") return ReactDOM;
    if (name === "react-intl") return { IntlProvider };
    if (name === "components/virtualized-table") return VirtualizedTable;
    throw new Error(`Module ${name} is not available in the plugin runtime`);
  };

  let preferenceSession = null;
  function bindPreferences(root) {
    const refresh = () => {
      for (const element of root.querySelectorAll("[preference]")) {
        const value = Prefs.get(element.getAttribute("preference"), true);
        if (element.type === "checkbox" || element.localName === "checkbox") element.checked = Boolean(value);
        else if ("value" in element && value !== undefined) element.value = String(value);
      }
    };
    refresh();
    const save = (event) => {
      const element = event.target.closest("[preference]");
      if (!element || !root.contains(element)) return;
      const key = element.getAttribute("preference");
      const old = Prefs.get(key, true);
      let value = element.type === "checkbox" || element.localName === "checkbox" ? element.checked : element.value;
      if (value === undefined) return;
      if (typeof old === "number" || element.type === "number") {
        if (String(value).trim() === "") return;
        value = Number(value);
        if (!Number.isFinite(value)) return;
      }
      if (value !== old) Prefs.set(key, value, true);
    };
    // Save before the plugin's command handlers inspect dependent preferences.
    root.addEventListener("input", save, true);
    root.addEventListener("change", save, true);
    root.addEventListener("command", save, true);
    return refresh;
  }

  function paneHandler(source) {
    // Single-expression hooks often return a promise. Await it before showing
    // dependent values, and still accept ordinary inline event-handler bodies.
    try {
      return new Function("window", "document", "event", `return (${source.replace(/;\s*$/, "")})`);
    } catch (error) {
      if (!(error instanceof SyntaxError)) throw error;
      return new Function("window", "document", "event", source);
    }
  }
  function closePreferences() {
    dismissConfirm?.();
    const session = preferenceSession;
    if (!session) return;
    for (const close of [...dialogClosers].reverse()) close();
    preferenceSession = null;
    for (const element of session.root.querySelectorAll("*")) element.dispatchEvent(new Event("unload"));
    session.root.remove();
    if (showing === session.root) {
      showing = null;
      send({ type: "hide" });
    }
  }
  async function openPreferences() {
    closePreferences();
    const root = document.createElement("div");
    root.className = "preference-pane";
    preferenceSession = { root };
    document.body.append(root);
    try {
      if (prefPane.src) root.append(...paneMarkup(fileText(prefPane.src)));
      for (const path of prefPane.stylesheets || []) {
        const style = document.createElement("style");
        style.textContent = fileText(resourceURL(path, prefPane.src));
        root.append(style);
      }
      for (const link of root.querySelectorAll('link[rel="stylesheet"]')) {
        const style = document.createElement("style");
        style.textContent = fileText(resourceURL(link.getAttribute("href"), prefPane.src));
        link.replaceWith(style);
      }
      for (const element of root.querySelectorAll("[data-l10n-id]")) localizeElement(element);
      const refresh = bindPreferences(root);
      // Keep hooks explicit: browsers don't fire load on XUL boxes. Removing
      // native handler attributes also prevents double invocation by WebKit.
      const loads = [...root.querySelectorAll("[onload]")].map((element) => {
        const source = element.getAttribute("onload");
        element.removeAttribute("onload");
        const handler = paneHandler(source);
        return () => {
          const event = new Event("load");
          Object.defineProperty(event, "target", { value: element });
          return handler.call(element, window, document, event);
        };
      });
      for (const element of root.querySelectorAll("*")) {
        for (const { name } of [...element.attributes]) {
          if (!/^on/.test(name)) continue;
          const source = element.getAttribute(name);
          element.removeAttribute(name);
          const handler = paneHandler(source);
          element.addEventListener(name.slice(2), (event) => {
            try { Promise.resolve(handler.call(element, window, document, event)).catch(report); }
            catch (error) { report(error); }
          });
        }
      }
      for (const script of root.querySelectorAll("script")) {
        if (script.getAttribute("src")) loadSubScript(resourceURL(script.getAttribute("src"), prefPane.src), window);
        else (0, eval)(script.textContent);
        script.remove();
      }
      for (const script of prefPane.scripts || []) loadSubScript(resourceURL(script, prefPane.src), window);
      show(root, "settings");
      for (const load of loads) await load();
      for (const element of root.querySelectorAll("*")) element.dispatchEvent(new Event("load"));
      // Load hooks may populate menus and set selectedIndex. Stored prefs win.
      refresh();
      for (const element of root.querySelectorAll("*")) element.dispatchEvent(new Event("showing"));
    } catch (error) {
      closePreferences();
      throw error;
    }
  }

  let plugin = null;

  async function start(message) {
    for (const file of message.code.files || []) files.set(file.path, file.text);
    readLocales();
    prefs = message.code.prefs ? JSON.parse(message.code.prefs) : {};
    readDefaultPrefs();
    setItems(message.items || []);
    itemsSeq = message.seq ?? -1;
    persistentKeys = message.selection?.itemKeys || [];
    selectionSeq = message.selection?.seq ?? -1;
    if (message.tabs) installTabs(message.tabs, false);
    const source = files.get("bootstrap.js");
    if (!source) throw new Error("The plugin has no bootstrap.js");
    plugin = new Function(
      "Zotero",
      "Services",
      "ChromeUtils",
      "Components",
      `${source}\nreturn {
        startup: typeof startup === "function" ? startup : null,
        shutdown: typeof shutdown === "function" ? shutdown : null,
        onMainWindowLoad: typeof onMainWindowLoad === "function" ? onMainWindowLoad : null,
        onMainWindowUnload: typeof onMainWindowUnload === "function" ? onMainWindowUnload : null,
      };`,
    )(Zotero, Services, ChromeUtils, Components);
    const data = { id: message.manifest.id, version: message.manifest.version, rootURI: ROOT, resourceURI: ROOT };
    if (plugin.startup) await plugin.startup(data, 1);
    const readerTabs = Zotero_Tabs._tabs.filter((tab) => tab.type === "reader").map((tab) => tab.id);
    if (readerTabs.length) void notify("add", "tab", readerTabs);
    // The main window is already open, and Zotero leaves a plugin started
    // then to set it up in startup; onMainWindowLoad is for later windows.
    send({ type: "ready" });
  }

  let showing = null;
  function show(element, kind) {
    for (const child of document.body.children) if (child !== element) child.hidden = true;
    element.hidden = false;
    showing = element;
    send({ type: "show", kind });
  }

  window.addEventListener("message", async (event) => {
    if (event.source !== app_) return;
    const message = event.data || {};
    try {
      switch (message.type) {
        case "start":
          await start(message);
          break;
        case "reply": {
          const pending = waiting.get(message.id);
          if (!pending) break;
          waiting.delete(message.id);
          if (message.ok) pending.resolve(message.value);
          else pending.reject(new Error(message.error));
          break;
        }
        case "items": {
          if (message.seq !== undefined && message.seq <= itemsSeq) break;
          if (message.seq !== undefined) itemsSeq = message.seq;
          const removed = setItems(message.items || []);
          const dispatched = [];
          for (const record of message.events || []) {
            const ids = (record.itemKeys || []).map((key) => itemIDs.get(key)).filter((id) => id !== undefined);
            const extraData = { ...(record.extraData || {}) };
            for (const key of record.itemKeys || []) {
              if (record.event === "delete") extraData[itemIDs.get(key)] = {
                libraryID: 1, key, ...(extraData[itemIDs.get(key)] || {}),
              };
              if (record.changed?.[key]) extraData[itemIDs.get(key)] = {
                changed: Object.fromEntries(record.changed[key].map((field) => [field, true])),
              };
            }
            if (ids.length) dispatched.push(notify(record.event, record.objectType, ids, extraData));
          }
          void Promise.all(dispatched).then(() => {
            for (const item of removed) if (!byKey.has(item.key)
              && !Zotero.Reader._readers.some((reader) => reader.itemID === item.id || reader._item.parentItemID === item.id)) byId.delete(item.id);
          });
          break;
        }
        case "selection": {
          if (message.seq !== undefined && message.seq <= selectionSeq) break;
          if (message.seq !== undefined) selectionSeq = message.seq;
          persistentKeys = message.itemKeys || [];
          break;
        }
        case "notifier": {
          if (message.seq !== undefined && message.seq <= notifierSeq) break;
          if (message.seq !== undefined) notifierSeq = message.seq;
          for (const record of message.events || []) {
            const ids = (record.itemKeys || []).map((key) => byKey.get(key)).filter((item) => item?.isAttachment()).map((item) => item.id);
            if (record.event === "open" && record.objectType === "file" && ids.length) void notify("open", "file", ids, record.extraData || {});
          }
          break;
        }
        case "tabs":
          installTabs(message);
          break;
        case "column-values": {
          const registered = columns.get(message.key);
          const values = Object.create(null);
          let reported = false;
          for (const [index, itemKey] of (message.itemKeys || []).entries()) {
            values[itemKey] = "";
            const item = byKey.get(itemKey);
            if (!registered || !item) continue;
            const { column, dataKey } = registered;
            try {
              let data = typeof column.dataProvider === "function"
                ? await column.dataProvider(item, dataKey)
                : typeof item.getField === "function" ? item.getField(dataKey) : "";
              data = String(data ?? "");
              if (typeof column.dataProvider !== "function" && typeof column.renderCell === "function") {
                const cell = column.renderCell(index, data, column, false, document);
                data = String(cell?.textContent ?? "");
              }
              values[itemKey] = data.slice(0, 500);
            } catch (error) {
              if (!reported) {
                report(error);
                reported = true;
              }
            }
          }
          send({ type: "column-values", id: message.id, values });
          break;
        }
        case "reader-release": {
          const view = readerViews.get(message.id);
          if (view) { clearInterval(view.timer); view.root.remove(); readerViews.delete(message.id); }
          break;
        }
        case "reader-event": {
          const view = readerViews.get(message.id);
          const node = view?.nodes.get(message.node);
          if (!node || !view.doc.body.contains(node)) break;
          if (!["click", "dblclick", "input", "change", "keydown"].includes(message.event)) break;
          if (typeof message.value === "string" && "value" in node) node.value = message.value;
          if (typeof message.checked === "boolean" && "checked" in node) node.checked = message.checked;
          const EventType = message.event === "keydown" ? KeyboardEvent : message.event.includes("click") ? MouseEvent : Event;
          node.dispatchEvent(new EventType(message.event, { bubbles: true, cancelable: true,
            key: String(message.key || ""), ctrlKey: Boolean(message.ctrlKey), metaKey: Boolean(message.metaKey), shiftKey: Boolean(message.shiftKey) }));
          break;
        }
        case "reader-render": {
          const bodies = [];
          let doc = document;
          let view;
          let target;
          if (message.live) {
            // Keep reader nodes laid out in the sandbox, as for in-page dialogs.
            // Detached documents have zero scrollHeight and collapse autosized UI.
            const root = document.createElement("div");
            root.style.cssText = "position:fixed;left:-10000px;top:0;width:440px;visibility:hidden;pointer-events:none";
            document.body.append(root);
            doc = Object.assign(new EventTarget(), {
              body: root, documentElement: root, defaultView: window, ownerGlobal: window,
              createElement: document.createElement.bind(document),
              createElementNS: document.createElementNS.bind(document),
              createTextNode: document.createTextNode.bind(document),
              createDocumentFragment: document.createDocumentFragment.bind(document),
              createXULElement: document.createXULElement.bind(document),
              querySelector: root.querySelector.bind(root),
              querySelectorAll: root.querySelectorAll.bind(root),
              getElementById: (id) => [...root.querySelectorAll("[id]")].find((element) => element.id === String(id)) || null,
              l10n: document.l10n,
            });
            target = doc.createElement("div");
            if (message.event === "renderTextSelectionPopup") {
              target.className = "selection-popup";
              const colors = doc.createElement("div");
              colors.className = "colors";
              target.append(colors);
            }
            doc.body.append(target);
            view = { doc, root, nodes: new Map(), ids: new WeakMap(), next: 1, html: null, timer: null };
            readerViews.set(message.id, view);
          }
          let reported = false;
          for (const handler of [...(readerListeners.get(message.event)?.keys() || [])]) {
            const body = doc.createElement("div");
            if (target) target.append(body);
            bodies.push(body);
            try {
              await handler({
                reader: readerFor(message), doc, params: message.params || {},
                append: (...nodes) => body.append(...nodes),
              });
            } catch (error) {
              if (!reported) {
                report(error);
                reported = true;
              }
            }
          }
          if (view) {
            const publish = () => {
              // Form values are properties, so a MutationObserver alone misses
              // asynchronous translations written to a textarea's value.
              const nodes = [target, ...target.querySelectorAll("*")];
              view.nodes.clear();
              const copy = target.cloneNode(true);
              const copies = [copy, ...copy.querySelectorAll("*")];
              nodes.forEach((node, index) => {
                let id = view.ids.get(node);
                if (!id) { id = String(view.next++); view.ids.set(node, id); }
                view.nodes.set(id, node);
                copies[index].setAttribute("data-reader-node", id);
                if (node.localName === "textarea") copies[index].textContent = node.value;
                if (node.localName === "input") {
                  copies[index].setAttribute("value", node.value);
                  copies[index].toggleAttribute("checked", node.checked);
                }
                if (node.localName === "option") copies[index].toggleAttribute("selected", node.selected);
              });
              const html = copy.innerHTML;
              if (html !== view.html) { view.html = html; send({ type: "reader-rendered", id: message.id, html }); }
            };
            if (readerViews.get(message.id) === view) {
              publish();
              view.timer = setInterval(publish, 250);
            }
            break;
          }
          await new Promise((resolve) => setTimeout(resolve, 50));
          send({ type: "reader-rendered", id: message.id, html: bodies.map((body) => body.innerHTML).join("") });
          break;
        }
        case "reader-menu": {
          const options = [];
          readerMenus.set(message.id, options);
          while (readerMenus.size > 20) readerMenus.delete(readerMenus.keys().next().value);
          let reported = false;
          for (const handler of [...(readerListeners.get(message.event)?.keys() || [])]) {
            try {
              await handler({
                reader: readerFor(message), params: message.params || {},
                append: (...items) => {
                  for (const item of items) {
                    if (item && typeof item.label === "string") options.push(item);
                  }
                },
              });
            } catch (error) {
              if (!reported) {
                report(error);
                reported = true;
              }
            }
          }
          send({ type: "reader-menu", id: message.id, items: options.map((option, index) => ({
            index, label: option.label, disabled: Boolean(option.disabled),
          })) });
          break;
        }
        case "reader-command": {
          const option = readerMenus.get(message.menu)?.[message.index];
          if (option && !option.disabled && typeof option.onCommand === "function") await option.onCommand();
          break;
        }
        case "item-menu": {
          const selection = { keys: message.itemKeys || [] };
          selections.push(selection);
          let result;
          try {
            result = readItemMenu();
          } finally {
            selections.splice(selections.indexOf(selection), 1);
          }
          itemMenus.set(message.id, result.options);
          while (itemMenus.size > 200) itemMenus.delete(itemMenus.keys().next().value);
          send({ type: "item-menu", id: message.id, items: result.items });
          break;
        }
        case "item-command": {
          const selection = { keys: message.itemKeys || [] };
          selections.push(selection);
          try {
            const path = message.path;
            if (!Array.isArray(path) || ![1, 2].includes(path.length) || path.some((label) => typeof label !== "string" || !label.trim())) {
              report(new Error("Item menu command has an invalid path"));
              break;
            }
            let element = itemMenus.get(message.menu)?.[message.index];
            if (!samePath(commandPath(element), path)) {
              const matches = readItemMenu().options.filter((option) => samePath(commandPath(option), path));
              if (matches.length !== 1) {
                report(new Error(`Item menu command ${JSON.stringify(path)} is ${matches.length ? "ambiguous" : "missing or disabled"}`));
                break;
              }
              element = matches[0];
            }
            if (element.registration) {
              await element.menu.onCommand?.(new Event("command"), menuContext(element.menuElem));
            } else {
              const event = new Event("command", { bubbles: true });
              event._pluginPromises = [];
              element.dispatchEvent(event);
              await Promise.all(event._pluginPromises);
            }
          } finally { selections.splice(selections.indexOf(selection), 1); }
          break;
        }
        case "render-section": {
          // Drawn here, in the plugin's own page; the app shows what came out.
          const section = sections.get(message.paneID);
          const body = document.createElement("div");
          const item = byKey.get(message.itemKey);
          let enabled = true;
          const props = {
            body,
            item,
            paneID: message.paneID,
            doc: document,
            tabType: "library",
            editable: false,
            setL10nArgs: () => {},
            setSectionSummary: () => {},
            setEnabled: (value) => { enabled = Boolean(value); },
            setSectionButtonStatus: () => {},
          };
          try {
            if (section && item) {
              if (section.onInit) section.onInit(props);
              if (section.onItemChange) section.onItemChange(props);
              if (enabled && section.onRender) section.onRender(props);
              if (enabled && section.onAsyncRender) await section.onAsyncRender(props);
            }
            await new Promise((resolve) => setTimeout(resolve, 50));
          } catch (error) {
            report(error);
          }
          const html = body.innerHTML;
          // The host owns a sanitized snapshot; this sandbox section is now unmounted.
          if (section?.onDestroy && item) {
            try { await section.onDestroy(props); } catch (error) { report(error); }
          }
          send({ type: "rendered", id: message.id, html });
          break;
        }
        case "settings":
          if (prefPane) await openPreferences();
          break;
        case "settings-close":
          closePreferences();
          if (showing) showing.remove();
          showing = null;
          send({ type: "hide" });
          break;
        case "stop":
          closePreferences();
          try {
            if (plugin && plugin.shutdown) await plugin.shutdown({ id: "", version: "", rootURI: ROOT }, 2);
          } finally {
            plugin = null;
            for (const view of readerViews.values()) { clearInterval(view.timer); view.root.remove(); }
            readerViews.clear();
            readerMenus.clear();
            itemMenus.clear();
            menuRegistrations.clear();
            persistentKeys = [];
            selections.length = 0;
            if (readerListeners.size) {
              readerListeners.clear();
              readerEventsChanged();
            }
          }
          break;
      }
    } catch (error) {
      report(error);
    }
  });

  send({ type: "loaded" });
})();
