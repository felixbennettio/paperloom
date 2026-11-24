// The Obsidian plugin runtime: runs one community plugin inside a sandboxed
// frame of its own (an opaque origin, no access to the app) and gives it the
// `obsidian` module. Whatever the plugin does to the vault, the editor or the
// network goes to the app as a message, which the app may refuse.
//
// Messages from the app: start, reply, command, ribbon, settings, stop,
// files. Messages to the app: call (answered by a reply), ready, error.
// The same file serves the Tauri app and the Apple apps.

(() => {
  "use strict";

  const app_ = window.parent;

  // A sandboxed frame has no storage of its own (reading it throws); a
  // plugin gets one that lasts while it runs.
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

  /** Waits until what a plugin draws stops changing — renderers that wait
   * for an index (Dataview's) draw again when it is ready — but no longer
   * than the app waits for an answer. */
  function settled(element, quiet = 300, most = 3500) {
    return new Promise((resolve) => {
      let timer = null;
      let cap = null;
      const observer = new MutationObserver(() => {
        clearTimeout(timer);
        timer = setTimeout(done, quiet);
      });
      function done() {
        observer.disconnect();
        clearTimeout(timer);
        clearTimeout(cap);
        resolve();
      }
      observer.observe(element, { childList: true, subtree: true, characterData: true, attributes: true });
      timer = setTimeout(done, quiet);
      cap = setTimeout(done, most);
    });
  }

  // IndexedDB is refused to a sandboxed frame too. Plugins that cache with it
  // (Dataview through localForage, others through Dexie) get one in memory,
  // for as long as they run: databases, object stores (key paths and
  // generated keys), get/getAll/put/add/delete/clear/count, cursors and key
  // ranges, with requests answered asynchronously as browsers do.
  const memoryIndexedDB = (() => {
    const databases = new Map();
    const later = (fn) => setTimeout(fn, 0);
    const clone = (value) => (typeof structuredClone === "function" ? structuredClone(value) : value);
    const rank = (key) =>
      typeof key === "number" ? 1 : key instanceof Date ? 2 : typeof key === "string" ? 3 : Array.isArray(key) ? 5 : 4;
    function compare(a, b) {
      const ra = rank(a);
      const rb = rank(b);
      if (ra !== rb) return ra < rb ? -1 : 1;
      if (ra === 5) {
        for (let i = 0; i < Math.min(a.length, b.length); i++) {
          const c = compare(a[i], b[i]);
          if (c) return c;
        }
        return a.length === b.length ? 0 : a.length < b.length ? -1 : 1;
      }
      const x = ra === 2 ? a.getTime() : a;
      const y = rb === 2 ? b.getTime() : b;
      return x === y ? 0 : x < y ? -1 : 1;
    }
    const failure = (name, message) => Object.assign(new Error(message), { name });
    class KeyRange {
      constructor(lower, upper, lowerOpen, upperOpen) {
        Object.assign(this, { lower, upper, lowerOpen: !!lowerOpen, upperOpen: !!upperOpen });
      }
      includes(key) {
        if (this.lower !== undefined) {
          const c = compare(key, this.lower);
          if (c < 0 || (c === 0 && this.lowerOpen)) return false;
        }
        if (this.upper !== undefined) {
          const c = compare(key, this.upper);
          if (c > 0 || (c === 0 && this.upperOpen)) return false;
        }
        return true;
      }
      static only(key) {
        return new KeyRange(key, key, false, false);
      }
      static lowerBound(key, open) {
        return new KeyRange(key, undefined, open, false);
      }
      static upperBound(key, open) {
        return new KeyRange(undefined, key, false, open);
      }
      static bound(lower, upper, lowerOpen, upperOpen) {
        return new KeyRange(lower, upper, lowerOpen, upperOpen);
      }
    }
    const inRange = (query, key) =>
      query === undefined || query === null ? true : query instanceof KeyRange ? query.includes(key) : compare(query, key) === 0;
    class Target {
      constructor() {
        this.listeners = {};
      }
      addEventListener(type, fn) {
        (this.listeners[type] = this.listeners[type] || []).push(fn);
      }
      removeEventListener(type, fn) {
        this.listeners[type] = (this.listeners[type] || []).filter((f) => f !== fn);
      }
      fire(type, extra = {}) {
        const event = { type, target: this, currentTarget: this, preventDefault() {}, stopPropagation() {}, ...extra };
        if (typeof this[`on${type}`] === "function") this[`on${type}`](event);
        for (const fn of this.listeners[type] || []) fn(event);
        return event;
      }
    }
    class Request extends Target {
      constructor(source, transaction) {
        super();
        Object.assign(this, { source, transaction, readyState: "pending", result: undefined, error: null });
      }
      succeed(result) {
        this.readyState = "done";
        this.result = result;
        this.fire("success");
      }
      fail(error) {
        this.readyState = "done";
        this.error = error;
        this.fire("error");
        if (this.transaction) this.transaction.abort(error);
      }
    }
    class Names {
      constructor(list) {
        this.list = list;
      }
      contains(name) {
        return this.list().includes(name);
      }
      item(i) {
        return this.list()[i] ?? null;
      }
      get length() {
        return this.list().length;
      }
      [Symbol.iterator]() {
        return this.list()[Symbol.iterator]();
      }
    }
    class Transaction extends Target {
      constructor(db, names, mode) {
        super();
        Object.assign(this, { db, mode, error: null, pending: 0, finished: false, aborted: false });
        this.objectStoreNames = new Names(() => names.slice());
        this.names = names;
        later(() => this.settle());
      }
      objectStore(name) {
        const allowed = this.mode === "versionchange" || this.names.includes(name);
        if (!allowed || !this.db.stores.has(name)) {
          throw failure("NotFoundError", `No object store named ${name}`);
        }
        return new Store(this, name);
      }
      run(store, work) {
        const request = new Request(store, this);
        this.pending++;
        later(() => {
          this.pending--;
          if (this.aborted) return;
          try {
            request.succeed(work());
          } catch (error) {
            request.fail(error);
          }
          this.settle();
        });
        return request;
      }
      settle() {
        later(() => {
          if (this.pending || this.finished || this.aborted) return;
          this.finished = true;
          this.fire("complete");
        });
      }
      abort(error) {
        if (this.finished || this.aborted) return;
        this.aborted = true;
        this.error = error || failure("AbortError", "The transaction was aborted");
        later(() => this.fire("abort"));
      }
      commit() {}
    }
    class Cursor {
      constructor(request, entries, valued) {
        Object.assign(this, { request, entries, valued, at: -1 });
      }
      step(by) {
        this.at += by;
        const entry = this.entries[this.at];
        later(() => {
          if (!entry) return this.request.succeed(null);
          this.key = this.primaryKey = entry[0];
          if (this.valued) this.value = clone(entry[1]);
          this.request.succeed(this);
        });
      }
      continue() {
        this.step(1);
      }
      advance(count) {
        this.step(count);
      }
      update(value) {
        return this.request.source.put(value, this.request.source.keyPath ? undefined : this.primaryKey);
      }
      delete() {
        return this.request.source.delete(this.primaryKey);
      }
    }
    class Store {
      constructor(transaction, name) {
        const data = transaction.db.stores.get(name);
        Object.assign(this, { transaction, name, data, keyPath: data.keyPath, autoIncrement: data.autoIncrement });
        this.indexNames = new Names(() => []);
      }
      /** A plain key's row, without sorting every row. */
      direct(query) {
        return (typeof query === "string" || typeof query === "number") && !(query instanceof KeyRange);
      }
      sorted(query, direction) {
        const entries = [...this.data.rows.entries()].filter(([key]) => inRange(query, key));
        entries.sort((a, b) => compare(a[0], b[0]));
        return direction && direction.startsWith("prev") ? entries.reverse() : entries;
      }
      keyOf(value, key) {
        if (this.keyPath) {
          let found = value;
          for (const part of String(this.keyPath).split(".")) found = found == null ? undefined : found[part];
          if (found === undefined && this.autoIncrement) {
            found = ++this.data.counter;
            let target = value;
            const parts = String(this.keyPath).split(".");
            for (const part of parts.slice(0, -1)) target = target[part] = target[part] || {};
            target[parts[parts.length - 1]] = found;
          }
          if (found === undefined) throw failure("DataError", "The value has no key");
          return found;
        }
        if (key === undefined && this.autoIncrement) return ++this.data.counter;
        if (key === undefined) throw failure("DataError", "No key was given");
        return key;
      }
      write(value, key, adding) {
        return this.transaction.run(this, () => {
          const stored = clone(value);
          const id = this.keyOf(stored, key);
          if (typeof id === "number" && id > this.data.counter) this.data.counter = Math.floor(id);
          if (adding && this.data.rows.has(id)) throw failure("ConstraintError", "The key already exists");
          // Dates and arrays are equal by value, not identity.
          if (typeof id === "object") {
            for (const existing of this.data.rows.keys()) {
              if (compare(existing, id) === 0) this.data.rows.delete(existing);
            }
          }
          this.data.rows.set(id, stored);
          return id;
        });
      }
      put(value, key) {
        return this.write(value, key, false);
      }
      add(value, key) {
        return this.write(value, key, true);
      }
      get(query) {
        return this.transaction.run(this, () => {
          if (this.direct(query)) return this.data.rows.has(query) ? clone(this.data.rows.get(query)) : undefined;
          const entry = this.sorted(query)[0];
          return entry ? clone(entry[1]) : undefined;
        });
      }
      getKey(query) {
        return this.transaction.run(this, () => this.sorted(query)[0]?.[0]);
      }
      getAll(query, count) {
        return this.transaction.run(this, () => this.sorted(query).slice(0, count || undefined).map((e) => clone(e[1])));
      }
      getAllKeys(query, count) {
        return this.transaction.run(this, () => this.sorted(query).slice(0, count || undefined).map((e) => e[0]));
      }
      delete(query) {
        return this.transaction.run(this, () => {
          if (this.direct(query)) this.data.rows.delete(query);
          else for (const [key] of this.sorted(query)) this.data.rows.delete(key);
        });
      }
      clear() {
        return this.transaction.run(this, () => this.data.rows.clear());
      }
      count(query) {
        return this.transaction.run(this, () => (query == null ? this.data.rows.size : this.sorted(query).length));
      }
      cursor(query, direction, valued) {
        const request = new Request(this, this.transaction);
        const cursor = new Cursor(request, this.sorted(query, direction), valued);
        cursor.step(1);
        return request;
      }
      openCursor(query, direction) {
        return this.cursor(query, direction, true);
      }
      openKeyCursor(query, direction) {
        return this.cursor(query, direction, false);
      }
      createIndex() {
        partial("IndexedDB indexes");
        return this;
      }
      index() {
        partial("IndexedDB indexes");
        return this;
      }
    }
    class Database extends Target {
      constructor(name) {
        super();
        Object.assign(this, { name, version: 0, stores: new Map() });
        this.objectStoreNames = new Names(() => [...this.stores.keys()].sort());
      }
      createObjectStore(name, options = {}) {
        if (this.stores.has(name)) throw failure("ConstraintError", `The object store ${name} exists`);
        this.stores.set(name, { rows: new Map(), counter: 0, keyPath: options.keyPath ?? null, autoIncrement: !!options.autoIncrement });
        return new Store(this.upgrading, name);
      }
      deleteObjectStore(name) {
        this.stores.delete(name);
      }
      transaction(names, mode = "readonly") {
        const list = Array.isArray(names) ? names : [names];
        for (const name of list) {
          if (!this.stores.has(name)) throw failure("NotFoundError", `No object store named ${name}`);
        }
        return new Transaction(this, list, mode);
      }
      close() {}
    }
    const factory = {
      open(name, version) {
        const request = new Request(null, null);
        later(() => {
          let db = databases.get(name);
          if (!db) databases.set(name, (db = new Database(name)));
          const wanted = version === undefined ? Math.max(db.version, 1) : version;
          if (wanted < db.version) {
            request.fail(failure("VersionError", "The database is newer"));
            return;
          }
          if (wanted > db.version) {
            const old = db.version;
            db.version = wanted;
            const upgrade = new Transaction(db, [], "versionchange");
            db.upgrading = upgrade;
            request.transaction = upgrade;
            request.result = db;
            request.readyState = "done";
            request.fire("upgradeneeded", { oldVersion: old, newVersion: wanted });
            request.transaction = null;
          }
          request.succeed(db);
        });
        return request;
      },
      deleteDatabase(name) {
        const request = new Request(null, null);
        later(() => {
          databases.delete(name);
          request.succeed(undefined);
        });
        return request;
      },
      databases: async () => [...databases.values()].map((db) => ({ name: db.name, version: db.version })),
      cmp: compare,
    };
    return { factory, KeyRange };
  })();
  for (const [name, value] of [
    ["indexedDB", memoryIndexedDB.factory],
    ["IDBKeyRange", memoryIndexedDB.KeyRange],
    // Obsidian's legacy editor global, where plugins register syntax modes.
    ["CodeMirror", inert()],
  ]) {
    try {
      Object.defineProperty(window, name, { value, configurable: true });
    } catch {
      // Left as it is.
    }
  }
  let nextCall = 1;
  const waiting = new Map();
  const send = (message) => app_.postMessage(message, "*");

  /** Asks the app to do something; its answer, or the reason it said no. */
  function call(method, ...args) {
    const id = nextCall++;
    return new Promise((resolve, reject) => {
      waiting.set(id, { resolve, reject });
      send({ type: "call", id, method, args });
    });
  }

  // Obsidian adds these helpers to every element; plugins build their
  // settings and modals with them.
  function decorate(element, options) {
    if (!options) return element;
    if (typeof options === "string") {
      element.textContent = options;
      return element;
    }
    if (options.cls) element.setAttribute("class", Array.isArray(options.cls) ? options.cls.join(" ") : options.cls);
    if (options.text !== undefined) element.textContent = String(options.text);
    if (options.title) element.title = options.title;
    if (options.href) element.setAttribute("href", options.href);
    if (options.type) element.setAttribute("type", options.type);
    if (options.value !== undefined) element.value = options.value;
    if (options.placeholder) element.setAttribute("placeholder", options.placeholder);
    if (options.attr) {
      for (const [k, v] of Object.entries(options.attr)) {
        if (v !== null && v !== undefined && v !== false) element.setAttribute(k, String(v));
      }
    }
    if (options.parent) {
      if (options.prepend) options.parent.insertBefore(element, options.parent.firstChild);
      else options.parent.appendChild(element);
    }
    return element;
  }
  const proto = HTMLElement.prototype;
  proto.createEl = function (tag, options, callback) {
    const element = decorate(document.createElement(tag), options);
    this.appendChild(element);
    if (callback) callback(element);
    return element;
  };
  proto.createDiv = function (options, callback) {
    return this.createEl("div", options, callback);
  };
  proto.createSpan = function (options, callback) {
    return this.createEl("span", options, callback);
  };
  proto.empty = function () {
    while (this.firstChild) this.removeChild(this.firstChild);
  };
  proto.setText = function (text) {
    this.textContent = String(text);
  };
  proto.addClass = function (...names) {
    this.classList.add(...names);
  };
  proto.removeClass = function (...names) {
    this.classList.remove(...names);
  };
  proto.toggleClass = function (name, on) {
    this.classList.toggle(name, on);
  };
  proto.hasClass = function (name) {
    return this.classList.contains(name);
  };
  proto.setAttr = function (name, value) {
    this.setAttribute(name, String(value));
  };
  proto.show = function () {
    this.style.display = "";
  };
  proto.hide = function () {
    this.style.display = "none";
  };
  /** An element not yet in the page. */
  const createEl = (tag, options, callback) => {
    const element = decorate(document.createElement(tag), options);
    if (callback) callback(element);
    return element;
  };
  window.createEl = createEl;
  window.createDiv = (options, callback) => createEl("div", options, callback);
  window.createSpan = (options, callback) => createEl("span", options, callback);

  // The rest of what Obsidian adds to nodes, elements and fragments (and to
  // a few built-in types), which plugins such as Dataview use throughout.
  const svgEl = (tag, options, callback) => {
    const element = decorate(document.createElementNS("http://www.w3.org/2000/svg", tag), options);
    if (callback) callback(element);
    return element;
  };
  window.createSvg = svgEl;
  window.createFragment = (callback) => {
    const fragment = document.createDocumentFragment();
    if (callback) callback(fragment);
    return fragment;
  };
  window.activeWindow = window;
  window.activeDocument = document;
  window.fish = (selector) => document.querySelector(selector);
  window.fishAll = (selector) => Array.from(document.querySelectorAll(selector));
  window.isBoolean = (value) => typeof value === "boolean";
  window.sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  window.nextFrame = () => new Promise((resolve) => requestAnimationFrame(() => resolve()));
  const define = (target, name, value) => {
    if (!(name in target)) Object.defineProperty(target, name, { value, configurable: true, writable: true });
  };
  const getter = (target, name, get) => {
    if (!(name in target)) Object.defineProperty(target, name, { get, configurable: true });
  };
  const nodeProto = Node.prototype;
  for (const name of ["createEl", "createDiv", "createSpan", "empty"]) define(nodeProto, name, proto[name]);
  define(nodeProto, "createSvg", function (tag, options, callback) {
    const element = svgEl(tag, options, callback);
    this.appendChild(element);
    return element;
  });
  define(nodeProto, "detach", function () {
    if (this.parentNode) this.parentNode.removeChild(this);
  });
  define(nodeProto, "appendText", function (text) {
    this.appendChild(document.createTextNode(String(text)));
  });
  define(nodeProto, "insertAfter", function (node, child) {
    this.insertBefore(node, child ? child.nextSibling : this.firstChild);
    return node;
  });
  define(nodeProto, "indexOf", function (other) {
    return Array.prototype.indexOf.call(this.childNodes, other);
  });
  define(nodeProto, "setChildrenInPlace", function (children) {
    while (this.firstChild) this.removeChild(this.firstChild);
    for (const child of children) this.appendChild(child);
  });
  define(nodeProto, "instanceOf", function (type) {
    return this instanceof type;
  });
  getter(nodeProto, "doc", function () {
    return this.ownerDocument || document;
  });
  getter(nodeProto, "win", () => window);
  getter(nodeProto, "constructorWin", () => window);
  const elementProto = Element.prototype;
  for (const name of ["setText", "addClass", "removeClass", "toggleClass", "hasClass", "setAttr"]) {
    define(elementProto, name, proto[name]);
  }
  define(elementProto, "getText", function () {
    return this.textContent || "";
  });
  define(elementProto, "addClasses", function (names) {
    this.classList.add(...names);
  });
  define(elementProto, "removeClasses", function (names) {
    this.classList.remove(...names);
  });
  define(elementProto, "setAttrs", function (attributes) {
    for (const [name, value] of Object.entries(attributes)) this.setAttribute(name, String(value));
  });
  define(elementProto, "getAttr", function (name) {
    return this.getAttribute(name);
  });
  define(elementProto, "matchParent", function (selector, last) {
    for (let element = this; element && element !== last; element = element.parentElement) {
      if (element.matches(selector)) return element;
    }
    return null;
  });
  define(elementProto, "getCssPropertyValue", function (property, pseudo) {
    return getComputedStyle(this, pseudo).getPropertyValue(property);
  });
  define(elementProto, "isActiveElement", function () {
    return document.activeElement === this;
  });
  for (const target of [elementProto, DocumentFragment.prototype, Document.prototype]) {
    define(target, "find", function (selector) {
      return this.querySelector(selector);
    });
    define(target, "findAll", function (selector) {
      return Array.from(this.querySelectorAll(selector));
    });
  }
  define(elementProto, "findAllSelf", function (selector) {
    const found = Array.from(this.querySelectorAll(selector));
    if (this.matches(selector)) found.unshift(this);
    return found;
  });
  define(proto, "toggle", function (show) {
    if (show) this.show();
    else this.hide();
  });
  define(proto, "toggleVisibility", function (visible) {
    this.toggle(visible);
  });
  define(proto, "isShown", function () {
    return this.style.display !== "none" && this.isConnected;
  });
  define(proto, "setCssStyles", function (styles) {
    Object.assign(this.style, styles);
  });
  define(proto, "setCssProps", function (props) {
    for (const [name, value] of Object.entries(props)) this.style.setProperty(name, value);
  });
  getter(proto, "innerWidth", function () {
    return this.clientWidth;
  });
  getter(proto, "innerHeight", function () {
    return this.clientHeight;
  });
  define(proto, "onClickEvent", function (listener, options) {
    this.addEventListener("click", listener, options);
    this.addEventListener("auxclick", listener, options);
  });
  define(proto, "onNodeInserted", function (listener, once) {
    let done = false;
    const check = () => {
      if (done || !this.isConnected) return;
      listener();
      if (once) done = true;
    };
    const observer = new MutationObserver(check);
    observer.observe(document, { childList: true, subtree: true });
    setTimeout(check, 0);
    return () => observer.disconnect();
  });
  define(proto, "trigger", function (type) {
    this.dispatchEvent(new Event(type, { bubbles: true }));
  });
  // Delegated listeners: el.on("click", ".row", (event, target) => …).
  const delegated = new WeakMap();
  define(proto, "on", function (type, selector, listener, options) {
    const handler = (event) => {
      const target = event.target instanceof Element ? event.target.closest(selector) : null;
      if (target && this.contains(target)) listener.call(this, event, target);
    };
    const list = delegated.get(this) || [];
    list.push({ type, selector, listener, handler });
    delegated.set(this, list);
    this.addEventListener(type, handler, options);
  });
  define(proto, "off", function (type, selector, listener, options) {
    const list = delegated.get(this) || [];
    for (const entry of list.filter((e) => e.type === type && e.selector === selector && e.listener === listener)) {
      this.removeEventListener(type, entry.handler, options);
      list.splice(list.indexOf(entry), 1);
    }
  });
  define(Array.prototype, "contains", function (item) {
    return this.includes(item);
  });
  define(Array.prototype, "remove", function (item) {
    const at = this.indexOf(item);
    if (at >= 0) this.splice(at, 1);
  });
  define(Array.prototype, "first", function () {
    return this[0];
  });
  define(Array.prototype, "last", function () {
    return this[this.length - 1];
  });
  define(Array.prototype, "unique", function () {
    return [...new Set(this)];
  });
  define(String.prototype, "contains", function (text) {
    return this.includes(text);
  });
  define(String.prototype, "format", function (...args) {
    return this.replace(/\{(\d+)\}/g, (match, i) => (args[i] !== undefined ? String(args[i]) : match));
  });
  define(Math, "clamp", (value, min, max) => Math.min(Math.max(value, min), max));
  define(Math, "square", (value) => value * value);
  define(Number, "isNumber", (value) => typeof value === "number");
  define(Object, "isEmpty", (value) => Object.keys(value).length === 0);
  define(Object, "each", (value, callback) => {
    for (const key of Object.keys(value)) if (callback(value[key], key) === false) return false;
    return true;
  });

  // Files, as plugins see them.
  class TAbstractFile {
    constructor(path) {
      this.path = path;
      this.name = path.split("/").pop();
      this.vault = vault;
    }
    get parent() {
      if (this.path === "") return null;
      const folder = this.path.includes("/") ? this.path.slice(0, this.path.lastIndexOf("/")) : "";
      return new TFolder(folder);
    }
  }
  class TFile extends TAbstractFile {
    constructor(path, stat) {
      super(path);
      this.extension = this.name.includes(".") ? this.name.slice(this.name.lastIndexOf(".") + 1) : "";
      this.basename = this.extension ? this.name.slice(0, -(this.extension.length + 1)) : this.name;
      this.stat = stat || { ctime: 0, mtime: 0, size: 0 };
    }
  }
  class TFolder extends TAbstractFile {
    /** The files and folders directly inside, folders first. */
    get children() {
      const prefix = this.path ? `${this.path}/` : "";
      const direct = (path) => path.startsWith(prefix) && path !== this.path && !path.slice(prefix.length).includes("/");
      const folders = [...folderPaths()].filter((path) => path && direct(path)).map((path) => new TFolder(path));
      return [...folders, ...files.filter((f) => direct(f.path))];
    }
    isRoot() {
      return this.path === "";
    }
  }

  class Events {
    constructor() {
      this._handlers = new Map();
    }
    on(name, callback, context) {
      const list = this._handlers.get(name) || [];
      const ref = { name, callback: context ? callback.bind(context) : callback, events: this };
      list.push(ref);
      this._handlers.set(name, list);
      if (name === "editor-menu") send({ type: "menu/register" });
      return ref;
    }
    off(name, callback) {
      this._handlers.set(name, (this._handlers.get(name) || []).filter((r) => r.callback !== callback));
    }
    offref(ref) {
      this._handlers.set(ref.name, (this._handlers.get(ref.name) || []).filter((r) => r !== ref));
    }
    trigger(name, ...args) {
      for (const ref of this._handlers.get(name) || []) {
        try {
          const result = ref.callback(...args);
          if (result?.catch) result.catch(report);
        } catch (error) {
          report(error);
        }
      }
    }
  }

  let files = [];
  /** The vault's folders, from its files' paths ("" is the top). */
  function folderPaths() {
    const found = new Set([""]);
    for (const file of files) {
      const parts = file.path.split("/");
      for (let i = 1; i < parts.length; i++) found.add(parts.slice(0, i).join("/"));
    }
    return found;
  }
  /** The folder at a path ("/" or "" for the top), or null. */
  function folderAt(path) {
    const at = normalizePath(path ?? "");
    return folderPaths().has(at) ? new TFolder(at) : null;
  }
  /** A file or folder, as Obsidian's getAbstractFileByPath finds it. */
  function fileOrFolder(path) {
    const at = normalizePath(path ?? "");
    return files.find((f) => f.path === at) || folderAt(at);
  }
  const vault = new Events();
  Object.assign(vault, {
    getName: () => "Vault",
    getRoot: () => new TFolder(""),
    getFiles: () => files.slice(),
    getMarkdownFiles: () => files.filter((f) => f.extension === "md"),
    getAllLoadedFiles: () => [...[...folderPaths()].map((path) => new TFolder(path)), ...files],
    getAbstractFileByPath: (path) => fileOrFolder(path),
    getFileByPath: (path) => files.find((f) => f.path === normalizePath(path)) || null,
    getFolderByPath: (path) => folderAt(path),
    read: (file) => call("vault.read", file.path),
    cachedRead: (file) => active && active.path === file.path ? Promise.resolve(active.value) : call("vault.read", file.path),
    modify: (file, text) => call("vault.write", file.path, text),
    append: async (file, text) => call("vault.write", file.path, (await call("vault.read", file.path)) + text),
    process: async (file, change) => {
      const next = change(await call("vault.read", file.path));
      await call("vault.write", file.path, next);
      return next;
    },
    create: async (path, text) => {
      await call("vault.create", normalizePath(path), text);
      return new TFile(normalizePath(path));
    },
    createFolder: (path) => call("vault.createFolder", normalizePath(path)),
    delete: (file) => call("vault.delete", file.path),
    trash: (file) => call("vault.delete", file.path),
    rename: (file, path) => call("vault.rename", file.path, normalizePath(path)),
  });
  vault.adapter = {
    read: (path) => call("vault.read", normalizePath(path)),
    write: (path, text) => call("vault.write", normalizePath(path), text),
    exists: async (path) => files.some((f) => f.path === normalizePath(path)),
    list: async (folder) => {
      const prefix = folder ? `${normalizePath(folder)}/` : "";
      const inside = files.filter((f) => f.path.startsWith(prefix)).map((f) => f.path.slice(prefix.length));
      return {
        files: inside.filter((p) => !p.includes("/")).map((p) => prefix + p),
        folders: [...new Set(inside.filter((p) => p.includes("/")).map((p) => prefix + p.split("/")[0]))],
      };
    },
  };

  const metadataCache = new Events();
  let caches = {};
  Object.assign(metadataCache, {
    fileToLinktext: (file, _source, omitExtension = false) => {
      const unique = files.filter(f => f.basename === file.basename).length <= 1;
      const path = unique ? file.name : file.path;
      return omitExtension ? path.replace(/\.md$/i, "") : path;
    },
    getFileCache: (file) => (file && caches[file.path]) || null,
    getCache: (path) => caches[path] || null,
    getFirstLinkpathDest: (link) => {
      const name = link.split("#")[0].split("|")[0].trim().toLowerCase();
      return files.find((f) => f.basename.toLowerCase() === name || f.path.toLowerCase() === name) || null;
    },
    get resolvedLinks() {
      return Object.fromEntries(Object.entries(caches).map(([path, cache]) => [path, cache.resolved || {}]));
    },
  });

  Object.defineProperties(metadataCache, {
    resolvedLinks: { get: () => Object.fromEntries(Object.entries(caches).map(([path, cache]) => [path, cache.resolved || {}])) },
    unresolvedLinks: { get: () => Object.fromEntries(Object.entries(caches).map(([path, cache]) => [path, cache.unresolved || {}])) },
  });

  // One laid-out mirror for each editor session in this plugin's own realm.
  const cm = globalThis.PaperloomCM6;
  const editorExtensions = [];
  const mirrors = new Map();
  const mirrorKey = snapshot => `${snapshot?.session}:${snapshot?.generation}`;
  const mirrorFor = snapshot => mirrors.get(mirrorKey(snapshot));
  const cmState = cm?.modules["@codemirror/state"];
  const cmView = cm?.modules["@codemirror/view"];
  const cmLanguage = cm?.modules["@codemirror/language"];
  const lineDecorationType = cmView && cmView.Decoration.line({}).constructor;
  function mirrorAttributes(input) {
    const output = {};
    for (const [name, value] of Object.entries(input || {}).slice(0, 30)) {
      if (typeof value !== "string" || value.length > 2000) continue;
      if ((name === "class" && /^[\w\s-]+$/.test(value)) || name === "title" || /^data-[a-z][a-z0-9-]*$/.test(name)) output[name] = value;
      else if (name === "style") output.style = value.split(";").filter(part => /^\s*(?:color|background(?:-color)?|text-decoration)\s*:/i.test(part) && !/[\\@]|url|var\(|expression|[<>]/i.test(part)).join(";");
    }
    return output;
  }
  function textSnapshot(element) {
    let text = "";
    const spans = [];
    function walk(node, depth = 0) {
      if (text.length > 20000 || spans.length >= 500 || depth > 30) return;
      if (node.nodeType === 3) {
        // Whitespace collapses as it does where the widget was drawn; the app's
        // editor keeps line breaks, which would split the widget over lines.
        let value = (node.nodeValue || "").replace(/\s+/g, " ");
        if (!text || text.endsWith(" ")) value = value.replace(/^ /, "");
        text += value.slice(0, 20000 - text.length);
        return;
      }
      if (node.nodeType !== 1 || /^(SCRIPT|STYLE|IFRAME|OBJECT|SVG)$/.test(node.tagName)) return;
      const from = text.length;
      for (const child of node.childNodes) walk(child, depth + 1);
      const attributes = mirrorAttributes(Object.fromEntries([...node.attributes].map(a => [a.name, a.value])));
      if (text.length > from && Object.keys(attributes).length && spans.length < 500) spans.push({ from, to: text.length, attributes });
    }
    walk(element);
    const trimmed = text.replace(/ $/, "");
    return { text: trimmed, spans: spans.map(span => ({ ...span, to: Math.min(span.to, trimmed.length) })).filter(span => span.to > span.from) };
  }
  function scheduleDecorations(mirror) {
    if (mirror.timer || mirror.closed) return;
    mirror.timer = setTimeout(() => { mirror.timer = null; exportDecorations(mirror); }, 16);
  }
  function exportDecorations(mirror) {
    if (mirror.closed) return;
    const decorations = [], used = new Set();
    try {
      for (const provider of mirror.view.state.facet(cmView.EditorView.decorations)) {
        const set = typeof provider === "function" ? provider(mirror.view) : provider;
        set?.between(0, mirror.view.state.doc.length, (from, to, value) => {
          if (decorations.length >= 10000) return;
          const spec = value.spec, attributes = mirrorAttributes({ ...spec.attributes, ...(spec.class ? { class: spec.class } : {}) });
          // Public decoration specs distinguish widgets/replacements; zero-width
          // decorations without a widget are line decorations.
          if (value.point) {
            if (value.constructor === lineDecorationType) {
              decorations.push({ kind: "line", from, to, attributes }); return;
            }
            let snapshot = { text: "", spans: [] };
            if (spec.widget) {
              used.add(spec.widget);
              let cached = mirror.widgets.get(spec.widget);
              if (!cached) {
                const dom = spec.widget.toDOM(mirror.view);
                // Some widgets (Dataview inline queries) return the same DOM
                // already mounted by CM. Never move or destroy CM-owned DOM.
                cached = { dom, owned: !dom.isConnected };
                mirror.widgets.set(spec.widget, cached);
                if (cached.owned) mirror.snapshots.appendChild(dom);
              }
              snapshot = textSnapshot(cached.dom);
            }
            partial("editor widgets: static text and approved spans; interactions and layout are not mirrored");
            decorations.push({ kind: from === to ? "widget" : "replace", from, to, attributes, partial: true, ...snapshot });
          } else decorations.push({ kind: "mark", from, to, attributes });
        });
      }
      for (const [widget, cached] of mirror.widgets) if (!used.has(widget)) {
        if (cached.owned) { cached.dom.remove(); widget.destroy?.(cached.dom); }
        mirror.widgets.delete(widget);
      }
      decorations.sort((a, b) => a.from - b.from || a.to - b.to);
      const result = { type: "decoration/result", session: mirror.snapshot.session, generation: mirror.snapshot.generation,
        revision: mirror.snapshot.revision, contextRevision: mirror.snapshot.contextRevision, decorations };
      const encoded = JSON.stringify(result);
      if (encoded !== mirror.last && encoded.length <= 2_000_000) { mirror.last = encoded; send(result); }
    } catch (error) { report(error); }
  }
  function mirrorConfiguration() { return [editorExtensions]; }
  function destroyMirror(mirror) {
    mirror.closed = true; clearTimeout(mirror.timer); mirror.observer.disconnect();
    mirror.view.destroy();
    for (const [widget, cached] of mirror.widgets) if (cached.owned) widget.destroy?.(cached.dom);
    mirror.container.remove(); mirrors.delete(mirrorKey(mirror.snapshot));
  }
  function syncMirror(snapshot) {
    if (!cm || !snapshot?.session) return;
    let mirror = mirrorFor(snapshot);
    if (!mirror) {
      const container = document.createElement("div");
      container.className = "paperloom-editor-mirror";
      // The iframe itself is offscreen. Keep local coordinates in its viewport
      // so CM can measure a real visible range; opacity preserves layout when
      // the frame is temporarily shown for plugin settings.
      container.style.cssText = "position:absolute;left:0;top:0;width:800px;height:600px;overflow:hidden;pointer-events:none;opacity:0;z-index:-1";
      container.setAttribute("aria-hidden", "true");
      document.body.appendChild(container);
      const snapshots = container.appendChild(document.createElement("div"));
      snapshots.style.cssText = "position:absolute;left:-10000px;top:0;width:800px";
      const compartment = new cmState.Compartment();
      mirror = { container, snapshots, compartment, snapshot, widgets: new Map(), closed: false, timer: null };
      const facade = new MarkdownView();
      const leaf = { view: facade, getViewState: () => ({ type: "markdown", state: { file: mirror.snapshot.path } }), rebuildView: () => workspace.updateOptions() };
      Object.assign(facade, { leaf, getMode: () => "source", getViewType: () => "markdown" });
      mirror.leaf = leaf;
      mirror.observer = new MutationObserver(() => scheduleDecorations(mirror));
      mirror.observer.observe(container, { subtree: true, childList: true, characterData: true, attributes: true });
      mirrors.set(mirrorKey(snapshot), mirror);
      const info = { file: new TFile(snapshot.path), editor: activeEditor.facade, view: facade };
      const state = cmState.EditorState.create({ doc: snapshot.value,
        selection: cmState.EditorSelection.create(snapshot.ranges.map(r => cmState.EditorSelection.range(r.anchor, r.head)), snapshot.mainIndex),
        extensions: [cmState.EditorState.allowMultipleSelections.of(true), cm.markdown.markdown(),
          cm.editorInfoField.init(() => info), cm.editorLivePreviewField.init(() => snapshot.mode === "live"),
          compartment.of(mirrorConfiguration()), cmView.EditorView.editable.of(false),
          cmView.EditorView.updateListener.of(() => scheduleDecorations(mirror))] });
      mirror.view = new cmView.EditorView({ state, parent: container });
      mirror.view.dom.style.height = "600px";
      mirror.view.scrollDOM.style.overflow = "auto";
    } else {
      const old = mirror.snapshot;
      mirror.snapshot = snapshot;
      const text = mirror.view.state.doc.toString();
      let prefix = 0, suffix = 0;
      while (prefix < text.length && prefix < snapshot.value.length && text[prefix] === snapshot.value[prefix]) prefix++;
      while (suffix < text.length - prefix && suffix < snapshot.value.length - prefix && text[text.length - 1 - suffix] === snapshot.value[snapshot.value.length - 1 - suffix]) suffix++;
      mirror.view.dispatch({ ...(text !== snapshot.value ? { changes: { from: prefix, to: text.length - suffix, insert: snapshot.value.slice(prefix, snapshot.value.length - suffix) } } : {}),
        selection: cmState.EditorSelection.create(snapshot.ranges.map(r => cmState.EditorSelection.range(r.anchor, r.head)), snapshot.mainIndex),
        effects: cm.contextEffect.of({ info: { file: new TFile(snapshot.path), editor: activeEditor.facade, view: mirror.leaf.view }, live: snapshot.mode === "live" }) });
      if (old.mode !== snapshot.mode) workspace.trigger("layout-change");
    }
    Object.assign(mirror.leaf.view, { editor: activeEditor.facade, file: new TFile(snapshot.path) });
    scheduleDecorations(mirror);
  }

  // Every facade owns a snapshot. In-flight callbacks never adopt a newer note
  // or revision: the host rejects their proposal instead of replaying callbacks.
  let active = null;
  let operation = 0;
  let commandEditor = null;
  let commandFile = null;
  let commandTail = Promise.resolve();
  let checkOnly = false;
  const copySnapshot = (snapshot) => snapshot ? {
    ...snapshot,
    ranges: (snapshot.ranges || [{ anchor: snapshot.from || 0, head: snapshot.to || 0 }]).map((r) => ({ ...r })),
    mainIndex: snapshot.mainIndex || 0,
  } : null;
  function makeEditor(snapshot, checking = false, batched = false) {
    const base = copySnapshot(snapshot);
    const local = copySnapshot(snapshot);
    const edits = [];
    let dirty = false, focus = false, scroll = false, queued = false, finished = false;
    const main = () => local.ranges[local.mainIndex];
    const clamp = (n, max) => Math.max(0, Math.min(Number.isFinite(n) ? Math.trunc(n) : 0, max));
    const offsetToPos = (offset) => {
      const before = local.value.slice(0, clamp(offset, local.value.length)).split("\n");
      return { line: before.length - 1, ch: before[before.length - 1].length };
    };
    const posToOffset = (pos) => {
      const lines = local.value.split("\n");
      const line = clamp(pos.line, lines.length - 1);
      return lines.slice(0, line).reduce((n, text) => n + text.length + 1, 0) + clamp(pos.ch, lines[line].length);
    };
    const writable = () => {
      if (checking || checkOnly) throw new Error("Editor mutations are refused during command/check");
      if (!local) throw new Error("No active editor");
      if (finished) throw new Error("Editor callback has already completed");
    };
    const submit = () => {
      if (finished) return;
      finished = true;
      if (!dirty || !base || base.protocol !== 1) return;
      send({ type: "editor/propose", operation: `editor-${++operation}`, session: base.session, generation: base.generation,
        baseRevision: base.revision, baseContextRevision: base.contextRevision, edits,
        ranges: local.ranges, mainIndex: local.mainIndex, focus, scroll });
    };
    const changed = () => {
      dirty = true;
      if (!batched && !queued) { queued = true; Promise.resolve().then(submit); }
    };
    const replace = (insert, from, to = from) => {
      writable();
      insert = String(insert);
      if (to < from) [from, to] = [to, from];
      const map = (n) => n < from ? n : n > to ? n + insert.length - (to - from) : from + insert.length;
      local.value = local.value.slice(0, from) + insert + local.value.slice(to);
      edits.push({ from, to, insert });
      local.ranges = local.ranges.map(({ anchor, head }) => ({ anchor: map(anchor), head: map(head) }));
      changed();
    };
    const facade = {
      getValue: () => local ? local.value : "",
      setValue: (text) => replace(text, 0, local.value.length),
      getSelection: () => local ? local.value.slice(Math.min(main().anchor, main().head), Math.max(main().anchor, main().head)) : "",
      getSelections: () => local ? local.ranges.map((r) => local.value.slice(Math.min(r.anchor, r.head), Math.max(r.anchor, r.head))) : [],
      somethingSelected: () => Boolean(local && local.ranges.some((r) => r.anchor !== r.head)),
      getLine: (n) => local ? local.value.split("\n")[n] || "" : "",
      lineCount: () => local ? local.value.split("\n").length : 0,
      firstLine: () => 0,
      lastLine: () => facade.lineCount() - 1,
      posToOffset,
      offsetToPos,
      getRange: (from, to) => local.value.slice(posToOffset(from), posToOffset(to)),
      getCursor: (which = "head") => {
        if (!local) return { line: 0, ch: 0 };
        const { anchor, head } = main();
        return offsetToPos(which === "from" || which === "start" ? Math.min(anchor, head)
          : which === "to" || which === "end" ? Math.max(anchor, head) : which === "anchor" ? anchor : head);
      },
      listSelections: () => local ? local.ranges.map((r) => ({ anchor: offsetToPos(r.anchor), head: offsetToPos(r.head) })) : [],
      setSelections: (ranges, mainIndex = 0) => {
        writable();
        if (!ranges.length || mainIndex < 0 || mainIndex >= ranges.length) throw new Error("Invalid editor selection");
        local.ranges = ranges.map((r) => ({ anchor: posToOffset(r.anchor), head: posToOffset(r.head) }));
        local.mainIndex = mainIndex;
        changed();
      },
      setSelection: (anchor, head = anchor) => facade.setSelections([{ anchor, head }]),
      setCursor: (pos, ch) => facade.setSelection(typeof pos === "number" ? { line: pos, ch: ch || 0 } : pos),
      replaceRange: (text, from, to = from) => replace(text, posToOffset(from), posToOffset(to)),
      replaceSelection: (text) => {
        writable();
        const from = Math.min(main().anchor, main().head), to = Math.max(main().anchor, main().head);
        replace(text, from, to);
        local.ranges = [{ anchor: from + String(text).length, head: from + String(text).length }];
        local.mainIndex = 0;
      },
      replaceSelections: (texts) => {
        writable();
        if (texts.length !== local.ranges.length) throw new Error("Selection count mismatch");
        const ranges = local.ranges.map((r, index) => ({ from: Math.min(r.anchor, r.head), to: Math.max(r.anchor, r.head), index }));
        for (const r of ranges.sort((a, b) => b.from - a.from)) replace(texts[r.index], r.from, r.to);
      },
      transaction: (transaction) => {
        writable();
        // Obsidian transaction changes all refer to the pre-transaction document.
        const changes = (transaction.changes || []).map((c) => ({ from: posToOffset(c.from), to: posToOffset(c.to || c.from), insert: c.text }));
        for (const c of changes.sort((a, b) => b.from - a.from)) replace(c.insert, c.from, c.to);
        if (transaction.selections) facade.setSelections(transaction.selections);
        else if (transaction.selection) facade.setSelection(transaction.selection.anchor, transaction.selection.head);
      },
      focus: () => { writable(); focus = true; changed(); },
      hasFocus: () => Boolean(local && local.focused),
      scrollIntoView: () => { writable(); scroll = true; changed(); },
    };
    // Commands keep their source snapshot; observational CM access uses its session mirror.
    Object.defineProperty(facade, "cm", { get: () => mirrorFor(base)?.view || { inputState: { composing: local?.composing ? 1 : 0 } } });
    return { facade, submit };
  }
  let activeEditor = makeEditor(null);
  const currentEditor = () => commandEditor || activeEditor.facade;
  function receiveSnapshot(snapshot) {
    if (!sameContext(active, snapshot)) {
      suggestEpoch++;
      editorMenus.clear();
      // Preserve trigger start while typing; candidate IDs are invalid immediately.
      if (suggestionSession) { if (suggestionSession.suggest !== selectingSuggest) suggestionSession.suggest.suggestions.values = []; suggestionSession = null; }
      if (!snapshot || snapshot.session !== active?.session || snapshot.generation !== active?.generation)
        for (const suggest of editorSuggesters) { if (suggest !== selectingSuggest) suggest.context = null; suggest.isOpen = false; }
    }
    active = copySnapshot(snapshot);
    activeEditor = makeEditor(active);
    if (active) syncMirror(active);
  }
  class MarkdownView {}
  function markdownLeaf() {
    if (!active && !commandEditor) return null;
    if (!commandEditor && mirrorFor(active)) return mirrorFor(active).leaf;
    return { view: Object.assign(new MarkdownView(), { editor: currentEditor(), file: workspace.getActiveFile(),
      getMode: () => "source" }), getViewState: () => ({ type: "markdown", state: { file: workspace.getActiveFile()?.path } }) };
  }
  const workspace = new Events();
  Object.assign(workspace, {
    getActiveFile: () => commandFile || (active ? files.find((f) => f.path === active.path) || new TFile(active.path) : null),
    getActiveViewOfType: (type) => ((active || commandEditor) && type === MarkdownView ? markdownLeaf()?.view : null),
    openLinkText: (link) => call("workspace.open", link),
    // A plugin's own views open in leaves of its own, shown over the app.
    getLeavesOfType: (type) => [...leaves, ...[...mirrors.values()].map(m => m.leaf)].filter((leaf) => leaf.view && leaf.view.getViewType() === type),
    getActiveLeaf: () => markdownLeaf(),
    getLastOpenFiles: () => active ? [active.path] : [],
    getLeaf: () => makeLeaf(),
    getRightLeaf: () => makeLeaf(),
    getLeftLeaf: () => makeLeaf(),
    revealLeaf: async (leaf) => {
      if (leaf && leaf.view) show(leaf.view.containerEl, "view");
    },
    detachLeavesOfType: (type) => {
      for (const leaf of workspace.getLeavesOfType(type)) leaf.detach();
    },
    iterateAllLeaves: (callback) => [...leaves, ...[...mirrors.values()].map(m => m.leaf)].forEach(callback),
    iterateRootLeaves: (callback) => leaves.forEach(callback),
    onLayoutReady: (callback) => setTimeout(callback, 0),
    get layoutReady() {
      return true;
    },
    // The app arranges its own panes; these keep plugins that ask about
    // Obsidian's layout going.
    get activeLeaf() {
      return markdownLeaf();
    },
    getMostRecentLeaf: () => markdownLeaf(),
    getLeafById: () => null,
    getGroupLeaves: () => [],
    setActiveLeaf: () => {},
    ensureSideLeaf: async () => makeLeaf(),
    requestSaveLayout: () => {},
    changeLayout: async () => {},
    // Re-read mutable extension arrays, retaining compartment and editor identity.
    updateOptions: () => {
      for (const mirror of mirrors.values()) {
        mirror.view.dispatch({ effects: mirror.compartment.reconfigure(mirrorConfiguration()) });
        scheduleDecorations(mirror);
      }
    },
    leftSplit: { collapsed: true, expand() {}, collapse() {}, toggle() {} },
    rightSplit: { collapsed: true, expand() {}, collapse() {}, toggle() {} },
    rootSplit: {},
  });
  Object.defineProperty(workspace, "activeLeaf", { get: () => markdownLeaf() });
  Object.defineProperty(workspace, "activeEditor", {
    get: () => active || commandEditor ? { editor: currentEditor(), file: workspace.getActiveFile() } : null,
  });
  const app = {
    vault,
    metadataCache,
    workspace,
    fileManager: {},
    plugins: { plugins: {} },
    loadLocalStorage: (key) => {
      const value = memoryStorage.getItem(key);
      return value === null ? null : JSON.parse(value);
    },
    saveLocalStorage: (key, value) => {
      if (value === null || value === undefined) memoryStorage.removeItem(key);
      else memoryStorage.setItem(key, JSON.stringify(value));
    },
  };
  vault.configDir = ".obsidian";
  vault.config = { useMarkdownLinks: false, newLinkFormat: "shortest" };
  vault.adapter.getResourcePath = (path) => path;
  vault.adapter.getBasePath = () => "";
  vault.getConfig = key => vault.config[key];
  workspace.containerEl = document.createElement("div");
  app.fileManager.generateMarkdownLink = (file, source, subpath = "", alias) => {
    const path = metadataCache.fileToLinktext(file, source, true) + subpath;
    return `[[${path}${alias ? "|" + alias : ""}]]`;
  };
  app.fileManager.renameFile = (file, path) => vault.rename(file, path);
  app.fileManager.processFrontMatter = async (file, change) => {
    const text = await vault.read(file);
    const match = /^---\n([\s\S]*?)\n---\n?/.exec(text);
    const front = {};
    if (match) {
      for (const line of match[1].split("\n")) {
        const at = line.indexOf(":");
        if (at > 0) front[line.slice(0, at).trim()] = line.slice(at + 1).trim();
      }
    }
    change(front);
    const lines = Object.entries(front).map(([k, v]) => `${k}: ${Array.isArray(v) ? `[${v.join(", ")}]` : v}`);
    const body = match ? text.slice(match[0].length) : text;
    await vault.modify(file, `---\n${lines.join("\n")}\n---\n${body}`);
  };

  // What the plugin added to the app.
  const commands = new Map();
  const codeProcessors = new Map();
  /** Markdown post processors, which change notes as they are shown. */
  const postProcessors = [];
  const addPostProcessor = (handler, sortOrder = 0) => {
    if (postProcessors.length === 0) send({ type: "postprocessor" });
    postProcessors.push({ handler, sortOrder: Number(sortOrder) || 0 });
    return handler;
  };

  /** Runs the post processors over a rendered note, each block in a
   * section of its own as Obsidian hands them over. */
  async function postProcess(container, path) {
    const context = {
      sourcePath: path,
      docId: path,
      frontmatter: (caches[path] || {}).frontmatter,
      addChild: (child) => child && child.load && child.load(),
      getSectionInfo: () => null,
    };
    const sections = [...container.children].map((child) => {
      const section = document.createElement("div");
      child.replaceWith(section);
      section.append(child);
      return section;
    });
    for (const { handler } of [...postProcessors].sort((a, b) => a.sortOrder - b.sortOrder)) {
      for (const section of sections) {
        try {
          await handler(section, context);
        } catch (error) {
          report(error);
        }
      }
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
    for (const section of sections) section.replaceWith(...section.childNodes);
  }
  const ribbon = [];
  let settingTab = null;

  class Component {
    constructor() {
      this._cleanups = [];
      this._children = [];
    }
    load() {
      this.onload();
    }
    onload() {}
    unload() {
      for (const child of this._children) child.unload();
      for (const clean of this._cleanups.splice(0)) {
        try {
          clean();
        } catch (error) {
          report(error);
        }
      }
      this.onunload();
    }
    onunload() {}
    addChild(child) {
      this._children.push(child);
      child.load();
      return child;
    }
    register(callback) {
      this._cleanups.push(callback);
    }
    registerEvent(ref) {
      this._cleanups.push(() => ref.events.offref(ref));
    }
    registerDomEvent(target, name, callback, options) {
      target.addEventListener(name, callback, options);
      this._cleanups.push(() => target.removeEventListener(name, callback, options));
    }
    registerInterval(id) {
      this._cleanups.push(() => clearInterval(id));
      return id;
    }
  }

  class Plugin extends Component {
    constructor(app_, manifest) {
      super();
      this.app = app_;
      this.manifest = manifest;
    }
    async loadData() {
      return startData === null ? null : JSON.parse(startData);
    }
    async saveData(data) {
      startData = JSON.stringify(data);
      await call("saveData", startData);
    }
    addCommand(command) {
      const full = { ...command, id: `${this.manifest.id}:${command.id}` };
      commands.set(full.id, full);
      send({ type: "commands", commands: [...commands.values()].map((c) => ({ id: c.id, name: c.name, editor: Boolean(c.editorCallback || c.editorCheckCallback) })) });
      return full;
    }
    removeCommand(id) {
      commands.delete(`${this.manifest.id}:${id}`);
    }
    addRibbonIcon(icon, title, callback) {
      ribbon.push(callback);
      send({ type: "ribbon", icon, title, index: ribbon.length - 1 });
      return document.createElement("div");
    }
    addStatusBarItem() {
      const item = document.createElement("div");
      const update = () => send({ type: "status", text: item.textContent || "" });
      new MutationObserver(update).observe(item, { childList: true, subtree: true, characterData: true });
      return item;
    }
    addSettingTab(tab) {
      settingTab = tab;
      send({ type: "settings-available" });
    }
    registerMarkdownPostProcessor(handler, sortOrder) {
      return addPostProcessor(handler, sortOrder);
    }
    registerMarkdownCodeBlockProcessor(language, handler) {
      codeProcessors.set(language, handler);
      send({ type: "codeblock", language });
      return { language };
    }
    registerEditorSuggest(suggest) {
      editorSuggesters.push(suggest);
      send({ type: "suggest/register" });
      this.register(() => { const i = editorSuggesters.indexOf(suggest); if (i >= 0) editorSuggesters.splice(i, 1); suggest.close(); });
    }
    registerEditorExtension(extension) {
      if (!cm) { partial("editor extensions"); return; }
      editorExtensions.push(extension); // Keep mutable arrays by reference.
      workspace.updateOptions();
      this.register(() => { const i = editorExtensions.indexOf(extension); if (i >= 0) editorExtensions.splice(i, 1); workspace.updateOptions(); });
    }
    registerView(type, factory) {
      viewFactories.set(type, factory);
    }
    registerExtensions() {}
    registerHoverLinkSource() {}
    registerObsidianProtocolHandler() {}
  }

  /** Something drawn into a note as it is shown, with its own lifetime. */
  class MarkdownRenderChild extends Component {
    constructor(containerEl) {
      super();
      this.containerEl = containerEl;
    }
  }

  /** Markdown drawn as the app draws notes, then through the post
   * processors. */
  const MarkdownRenderer = {
    async render(_app, markdown, el, sourcePath, _component) {
      el.innerHTML = await call("markdown.render", String(markdown ?? ""), String(sourcePath ?? ""));
      if (postProcessors.length) await postProcess(el, sourcePath);
    },
    async renderMarkdown(markdown, el, sourcePath, component) {
      await MarkdownRenderer.render(app, markdown, el, sourcePath, component);
    },
  };

  const MarkdownPreviewRenderer = {
    registerPostProcessor: (handler, sortOrder) => addPostProcessor(handler, sortOrder),
    unregisterPostProcessor(handler) {
      const at = postProcessors.findIndex((p) => p.handler === handler);
      if (at >= 0) postProcessors.splice(at, 1);
    },
  };

  /** A plugin's own view: shown over the app when its leaf is revealed. */
  class ItemView extends Component {
    constructor(leaf) {
      super();
      this.leaf = leaf;
      this.app = app;
      this.navigation = false;
      this.containerEl = document.createElement("div");
      this.containerEl.className = "workspace-leaf-content";
      this.containerEl.hidden = true;
      this.titleEl = this.containerEl.createEl("h2", { cls: "view-header-title" });
      this.contentEl = this.containerEl.createDiv({ cls: "view-content" });
    }
    getViewType() {
      return "";
    }
    getDisplayText() {
      return "";
    }
    getIcon() {
      return "";
    }
    async onOpen() {}
    async onClose() {}
    addAction(_icon, title, callback) {
      const button = this.titleEl.createEl("button", { text: title });
      button.addEventListener("click", callback);
      return button;
    }
  }

  const viewFactories = new Map();
  const leaves = [];
  function makeLeaf() {
    const leaf = {
      view: null,
      async setViewState(state) {
        const factory = viewFactories.get(state && state.type);
        if (!factory) return;
        if (leaf.view) {
          await leaf.view.onClose();
          leaf.view.containerEl.remove();
        }
        const view = factory(leaf);
        leaf.view = view;
        view.titleEl.textContent = view.getDisplayText();
        document.body.appendChild(view.containerEl);
        view.load();
        await view.onOpen();
        if (state.active !== false) show(view.containerEl, "view");
      },
      getViewState: () => ({ type: leaf.view ? leaf.view.getViewType() : "empty" }),
      async detach() {
        if (leaf.view) {
          await leaf.view.onClose();
          leaf.view.unload();
          leaf.view.containerEl.remove();
        }
        leaves.splice(leaves.indexOf(leaf), 1);
        hideAll();
      },
      openFile: (file) => call("workspace.open", file.path),
      setPinned() {},
      getDisplayText: () => (leaf.view ? leaf.view.getDisplayText() : ""),
    };
    leaves.push(leaf);
    return leaf;
  }

  class PluginSettingTab {
    constructor(app_, plugin) {
      this.app = app_;
      this.plugin = plugin;
      this.containerEl = document.body.createDiv({ cls: "setting-tab" });
      this.containerEl.hidden = true;
    }
    display() {}
    hide() {}
  }

  class Setting {
    constructor(container) {
      this.settingEl = container.createDiv({ cls: "setting-item" });
      this.infoEl = this.settingEl.createDiv({ cls: "setting-item-info" });
      this.nameEl = this.infoEl.createDiv({ cls: "setting-item-name" });
      this.descEl = this.infoEl.createDiv({ cls: "setting-item-description" });
      this.controlEl = this.settingEl.createDiv({ cls: "setting-item-control" });
      this.components = [];
    }
    setName(name) {
      this.nameEl.textContent = typeof name === "string" ? name : "";
      if (typeof name !== "string" && name) this.nameEl.appendChild(name);
      return this;
    }
    setDesc(desc) {
      this.descEl.textContent = typeof desc === "string" ? desc : "";
      if (typeof desc !== "string" && desc) this.descEl.appendChild(desc);
      return this;
    }
    setHeading() {
      this.settingEl.classList.add("setting-item-heading");
      return this;
    }
    setClass(name) {
      this.settingEl.classList.add(name);
      return this;
    }
    setDisabled(disabled) {
      this.settingEl.classList.toggle("is-disabled", disabled);
      return this;
    }
    then(callback) {
      callback(this);
      return this;
    }
    _add(component, callback) {
      this.components.push(component);
      if (callback) callback(component);
      return this;
    }
    addText(callback) {
      return this._add(new TextComponent(this.controlEl), callback);
    }
    addTextArea(callback) {
      return this._add(new TextAreaComponent(this.controlEl), callback);
    }
    addToggle(callback) {
      return this._add(new ToggleComponent(this.controlEl), callback);
    }
    addDropdown(callback) {
      return this._add(new DropdownComponent(this.controlEl), callback);
    }
    addSlider(callback) {
      return this._add(new SliderComponent(this.controlEl), callback);
    }
    addButton(callback) {
      return this._add(new ButtonComponent(this.controlEl), callback);
    }
    addExtraButton(callback) {
      return this._add(new ButtonComponent(this.controlEl), callback);
    }
  }

  class ValueComponent {
    constructor(element, event = "input") {
      this.inputEl = element;
      this._event = event;
    }
    getValue() {
      return this.inputEl.value;
    }
    setValue(value) {
      this.inputEl.value = value;
      return this;
    }
    setPlaceholder(text) {
      this.inputEl.placeholder = text;
      return this;
    }
    setDisabled(disabled) {
      this.inputEl.disabled = disabled;
      return this;
    }
    onChange(callback) {
      this.inputEl.addEventListener(this._event, () => callback(this.getValue()));
      return this;
    }
    then(callback) {
      callback(this);
      return this;
    }
  }
  class TextComponent extends ValueComponent {
    constructor(container) {
      super(container.createEl("input", { type: "text" }));
    }
  }
  class TextAreaComponent extends ValueComponent {
    constructor(container) {
      super(container.createEl("textarea"));
    }
  }
  class ToggleComponent extends ValueComponent {
    constructor(container) {
      super(container.createEl("input", { type: "checkbox", cls: "checkbox-toggle" }), "change");
      this.toggleEl = this.inputEl;
    }
    getValue() {
      return this.inputEl.checked;
    }
    setValue(on) {
      this.inputEl.checked = Boolean(on);
      return this;
    }
  }
  class DropdownComponent extends ValueComponent {
    constructor(container) {
      super(container.createEl("select", { cls: "dropdown" }), "change");
      this.selectEl = this.inputEl;
    }
    addOption(value, label) {
      this.inputEl.createEl("option", { value, text: label });
      return this;
    }
    addOptions(options) {
      for (const [value, label] of Object.entries(options)) this.addOption(value, label);
      return this;
    }
  }
  class SliderComponent extends ValueComponent {
    constructor(container) {
      super(container.createEl("input", { type: "range", cls: "slider" }));
      this.sliderEl = this.inputEl;
    }
    getValue() {
      return Number(this.inputEl.value);
    }
    setLimits(min, max, step) {
      Object.assign(this.inputEl, { min, max, step: step === "any" ? "any" : step });
      return this;
    }
    setDynamicTooltip() {
      return this;
    }
  }
  class ButtonComponent {
    constructor(container) {
      this.buttonEl = container.createEl("button");
      this.extraSettingsEl = this.buttonEl;
    }
    setButtonText(text) {
      this.buttonEl.textContent = text;
      return this;
    }
    setIcon(icon) {
      this.buttonEl.title = this.buttonEl.title || icon;
      if (!this.buttonEl.textContent) this.buttonEl.textContent = "•";
      return this;
    }
    setTooltip(text) {
      this.buttonEl.title = text;
      return this;
    }
    setCta() {
      this.buttonEl.classList.add("mod-cta");
      return this;
    }
    setWarning() {
      this.buttonEl.classList.add("mod-warning");
      return this;
    }
    setDisabled(disabled) {
      this.buttonEl.disabled = disabled;
      return this;
    }
    onClick(callback) {
      this.buttonEl.addEventListener("click", (event) => callback(event));
      return this;
    }
  }

  /** The frame shows one thing at a time: the plugin's settings or a modal. */
  let showing = null;
  function show(element, kind) {
    for (const child of document.body.children) if (child !== element) child.hidden = true;
    element.hidden = false;
    showing = element;
    send({ type: "show", kind });
  }
  function hideAll() {
    if (showing) showing.hidden = true;
    showing = null;
    send({ type: "hide" });
  }

  class Modal {
    constructor(app_) {
      this.app = app_;
      this.modalEl = document.body.createDiv({ cls: "modal" });
      this.modalEl.hidden = true;
      this.titleEl = this.modalEl.createEl("h2", { cls: "modal-title" });
      this.contentEl = this.modalEl.createDiv({ cls: "modal-content" });
      this.scope = { register() {} };
    }
    open() {
      show(this.modalEl, "modal");
      this.onOpen();
    }
    close() {
      this.onClose();
      this.modalEl.remove();
      hideAll();
    }
    setTitle(title) {
      this.titleEl.textContent = title;
      return this;
    }
    setContent(content) {
      this.contentEl.textContent = typeof content === "string" ? content : "";
      return this;
    }
    onOpen() {}
    onClose() {}
  }


  /** Key callbacks and suggestion values never leave this realm. */
  class Scope {
    constructor(parent) { this.parent = parent; this.keys = []; }
    register(modifiers, key, func) {
      const handler = { modifiers: modifiers === null ? null : [...modifiers], key, func };
      this.keys.push(handler);
      return handler;
    }
    unregister(handler) { const i = this.keys.indexOf(handler); if (i >= 0) this.keys.splice(i, 1); }
    handleKey(event) {
      const aliases = { Mod: Platform.isMacOS ? "Meta" : "Ctrl", Control: "Ctrl", Command: "Meta" };
      const actual = [event.ctrlKey && "Ctrl", event.metaKey && "Meta", event.altKey && "Alt", event.shiftKey && "Shift"].filter(Boolean).sort();
      const handler = this.keys.find(h => (!h.key || h.key.toLowerCase() === event.key.toLowerCase()) &&
        (h.modifiers === null || h.modifiers.map(m => aliases[m] || m).sort().join() === actual.join()));
      if (handler) { this.lastResult = handler.func(event, { key: event.key, modifiers: actual }); return true; }
      if (this.parent?.handleKey(event)) { this.lastResult = this.parent.lastResult; return true; }
      return false;
    }
  }
  const editorSuggesters = [];
  let suggestEpoch = 0, suggestionSession = null, selectingSuggest = null, queuedSuggestQuery = null, latestSuggestRequest = null;
  const sameContext = (a, b) => a && b && a.session === b.session && a.generation === b.generation &&
    a.revision === b.revision && a.contextRevision === b.contextRevision;
  const suggestIdentity = snapshot => ({ session: snapshot.session, generation: snapshot.generation,
    revision: snapshot.revision, contextRevision: snapshot.contextRevision });
  class PopoverSuggest {
    constructor(app) {
      this.app = app;
      this.scope = new Scope();
      this.containerEl = document.createElement("div");
      this.suggestEl = this.containerEl.createDiv();
      this.isOpen = false;
      this.limit = 100;
      this.suggestions = {
        values: [], selectedItem: 0, containerEl: this.suggestEl,
        setSelectedItem: (index) => {
          const count = this.suggestions.values.length;
          this.suggestions.selectedItem = count ? (index + count) % count : 0;
          if (suggestionSession?.suggest === this) publishSuggestions(suggestionSession);
        },
        useSelectedItem: (event) => this.selectSuggestion(this.suggestions.values[this.suggestions.selectedItem], event),
      };
      this.scope.register([], "Escape", () => { this.close(); return false; });
      for (const key of ["Enter", "Tab"]) this.scope.register([], key, event => { this.suggestions.useSelectedItem(event); return false; });
      this.scope.register([], "ArrowDown", () => { this.suggestions.setSelectedItem(this.suggestions.selectedItem + 1); return false; });
      this.scope.register([], "ArrowUp", () => { this.suggestions.setSelectedItem(this.suggestions.selectedItem - 1); return false; });
    }
    open() { this.isOpen = true; }
    close() {
      this.isOpen = false;
      if (selectingSuggest !== this) { this.context = null; this.suggestions.values = []; }
      if (suggestionSession?.suggest === this) {
        const old = suggestionSession;
        suggestionSession = null;
        suggestEpoch++;
        send({ type: "suggest/close", request: old.request, ...suggestIdentity(old.snapshot) });
      }
    }
    setInstructions(instructions) { this.instructions = instructions; }
  }
  class EditorSuggest extends PopoverSuggest {
    constructor(app) { super(app); this.context = null; }
    trigger() {
      // Manual plugin commands ask the host to start a revisioned query.
      send({ type: "suggest/trigger" });
    }
    onTrigger() { return null; }
    getSuggestions() { return []; }
    renderSuggestion(value, el) { el.setText(String(value)); }
    selectSuggestion() {}
  }
  function publishSuggestions(session) {
    send({ type: "suggest/result", request: session.request, ...suggestIdentity(session.snapshot),
      priority: Number.isFinite(session.suggest.priority) ? session.suggest.priority : 0,
      start: session.context.editor.posToOffset(session.context.start), end: session.context.editor.posToOffset(session.context.end),
      items: session.items, selected: session.suggest.suggestions.selectedItem,
      keys: session.suggest.scope.keys.filter(k => k.key && k.modifiers).slice(0, 100).map(k => ({ key: k.key, modifiers: k.modifiers })) });
  }
  async function querySuggestions(message) {
    // Keep this.context pinned during an async selection callback. New typing
    // still invalidates its proposal; only the latest query resumes afterward.
    const snapshot = copySnapshot(message.editor);
    if (!sameContext(active, snapshot) || snapshot.composing || !snapshot.focused) return;
    latestSuggestRequest = message.request;
    if (selectingSuggest) { queuedSuggestQuery = message; return; }
    ++suggestEpoch;
    const context = makeEditor(snapshot, true, true);
    const editor = context.facade, file = new TFile(snapshot.path);
    // Priority first, registration order breaks ties inside this frame.
    const ordered = editorSuggesters.map((suggest, order) => ({ suggest, order }))
      .sort((a, b) => (Number(b.suggest.priority) || 0) - (Number(a.suggest.priority) || 0) || a.order - b.order);
    let chosen = null;
    for (const { suggest } of ordered) {
      const trigger = suggest.onTrigger(editor.getCursor(), editor, file);
      if (!trigger) { suggest.context = null; suggest.isOpen = false; continue; }
      const start = editor.posToOffset(trigger.start), end = editor.posToOffset(trigger.end);
      if (start > end || end > snapshot.value.length) continue;
      chosen = { suggest, trigger };
      break;
    }
    if (!chosen) {
      if (suggestionSession) suggestionSession.suggest.close();
      send({ type: "suggest/result", request: message.request, ...suggestIdentity(snapshot), items: [] });
      return;
    }
    if (suggestionSession && suggestionSession.suggest !== chosen.suggest) suggestionSession.suggest.close();
    // Closing a previous popup above may advance the epoch.
    const currentEpoch = suggestEpoch;
    const { suggest, trigger } = chosen;
    suggest.context = { ...trigger, editor, file };
    const sourceContext = suggest.context;
    const values = await suggest.getSuggestions(sourceContext);
    if (currentEpoch !== suggestEpoch || !sameContext(active, snapshot) || suggest.context !== sourceContext) return;
    const limited = (Array.isArray(values) ? values : []).slice(0, Math.min(100, suggest.limit || 100));
    const items = limited.map((value, index) => {
      const el = document.createElement("div");
      suggest.renderSuggestion(value, el);
      // Export rendered text only, including a separate description when available.
      const content = el.firstElementChild;
      const label = content?.childElementCount > 1 ? content.firstElementChild.textContent : el.textContent;
      const secondary = content?.childElementCount > 1 ? [...content.children].slice(1).map(n => n.textContent).join(" ") : undefined;
      return { id: `${message.request}:${index}`, label: String(label || "").slice(0, 500), secondary: secondary?.slice(0, 500) };
    });
    suggest.suggestions.values = limited;
    suggest.suggestions.selectedItem = 0;
    if (limited.length) suggest.open();
    else suggest.isOpen = false;
    suggestionSession = { request: message.request, snapshot, suggest, context: sourceContext, items };
    publishSuggestions(suggestionSession);
  }
  async function chooseSuggestion(message) {
    const session = suggestionSession;
    if (selectingSuggest || !session || session.request !== message.request || !sameContext(active, session.snapshot) || !sameContext(message, session.snapshot)) return;
    const index = session.items.findIndex(item => item.id === message.id);
    if (index < 0) return;
    const suggest = session.suggest;
    selectingSuggest = suggest;
    const context = makeEditor(session.snapshot, false, true);
    suggest.context = { ...session.context, editor: context.facade };
    suggest.suggestions.selectedItem = index;
    const event = new KeyboardEvent("keydown", { key: message.key || "Enter", shiftKey: !!message.shiftKey,
      ctrlKey: !!message.ctrlKey, altKey: !!message.altKey, metaKey: !!message.metaKey });
    let selected = false;
    const selectionTasks = [];
    const use = suggest.suggestions.useSelectedItem;
    suggest.suggestions.useSelectedItem = evt => { selected = true; return suggest.selectSuggestion(suggest.suggestions.values[suggest.suggestions.selectedItem], evt); };
    try {
      // Scope handlers may call selectSuggestion directly as Various Complements does.
      const original = suggest.selectSuggestion;
      suggest.selectSuggestion = function(...args) { selected = true; const result = original.apply(this, args); selectionTasks.push(Promise.resolve(result)); return result; };
      try {
        if (suggest.scope.handleKey(event)) await suggest.scope.lastResult;
        else if (["Enter", "Tab"].includes(event.key)) await suggest.suggestions.useSelectedItem(event);
        // Keep async selection callbacks inside one proposal.
        await Promise.all(selectionTasks);
      } finally { suggest.selectSuggestion = original; }
    } finally {
      suggest.suggestions.useSelectedItem = use;
      context.submit();
      selectingSuggest = null;
      if (selected || suggestionSession !== session) { suggest.close(); suggest.context = null; suggest.suggestions.values = []; }
      else suggest.context = session.context;
      const queued = queuedSuggestQuery;
      queuedSuggestQuery = null;
      if (queued) await querySuggestions(queued);
    }
  }

  class MenuItem {
    constructor() { this.dom = document.createElement("div"); this.titleEl = this.dom.createDiv(); this.disabled = false; }
    setTitle(title) { this.titleEl.setText(String(title)); return this; }
    setIcon() { return this; }
    setDisabled(disabled) { this.disabled = !!disabled; return this; }
    setChecked() { return this; }
    setSection() { return this; }
    onClick(callback) { this.callback = callback; return this; }
    setSubmenu() { this.submenu = new Menu(); return this.submenu; }
  }
  class Menu {
    constructor() { this.items = []; this.dom = document.createElement("div"); }
    addItem(callback) { if (this.items.length >= 50) return this; const item = new MenuItem(); callback(item); this.items.push(item); this.dom.appendChild(item.dom); return this; }
    addSeparator() { if (this.items.length < 50) this.items.push({ separator: true }); return this; }
    setUseNativeMenu() { return this; }
    showAtPosition() { partial("standalone plugin menus"); }
    showAtMouseEvent(event) { return this.showAtPosition({ x: event.clientX, y: event.clientY }); }
    hide() {}
  }
  const editorMenus = new Map();
  function requestEditorMenu(message) {
    if (!sameContext(active, message.editor)) return;
    const context = makeEditor(message.editor, false, true);
    const menu = new Menu(), callbacks = [];
    workspace.trigger("editor-menu", menu, context.facade, { editor: context.facade, file: new TFile(message.editor.path) });
    let count = 0;
    const serialize = (menu, depth = 0) => menu.items.slice(0, 50).flatMap(item => {
      if (++count > 50) return [];
      if (item.separator) return [{ separator: true }];
      const label = item.titleEl.textContent.slice(0, 500);
      if (!label.trim()) return [];
      if (item.submenu && depth === 0) return [{ label, disabled: item.disabled, children: serialize(item.submenu, 1) }];
      const index = callbacks.length;
      callbacks.push(item);
      return [{ index, label, disabled: item.disabled }];
    });
    const items = serialize(menu);
    editorMenus.clear(); // One active context menu, callbacks bound to its source snapshot.
    editorMenus.set(message.request, { snapshot: copySnapshot(message.editor), context, callbacks });
    send({ type: "menu/result", request: message.request, ...suggestIdentity(message.editor), items });
  }
  async function chooseEditorMenu(message) {
    const menu = editorMenus.get(message.request);
    if (!menu || !sameContext(active, menu.snapshot) || !sameContext(message, menu.snapshot)) return;
    editorMenus.clear();
    const item = menu.callbacks[message.index];
    if (!item || item.disabled || !item.callback) return;
    try { await item.callback(new MouseEvent("click", { shiftKey: !!message.shiftKey, ctrlKey: !!message.ctrlKey, metaKey: !!message.metaKey, altKey: !!message.altKey })); }
    finally { menu.context.submit(); }
  }

  class SuggestModal extends Modal {
    constructor(app_) {
      super(app_);
      this.inputEl = this.contentEl.createEl("input", { type: "text", cls: "prompt-input" });
      this.resultContainerEl = this.contentEl.createDiv({ cls: "prompt-results" });
      this.limit = 50;
      this.inputEl.addEventListener("input", () => this._refresh());
    }
    setPlaceholder(text) {
      this.inputEl.placeholder = text;
    }
    setInstructions() {}
    async _refresh() {
      const found = await this.getSuggestions(this.inputEl.value);
      this.resultContainerEl.empty();
      for (const item of (found || []).slice(0, this.limit)) {
        const row = this.resultContainerEl.createDiv({ cls: "suggestion-item" });
        this.renderSuggestion(item, row);
        row.addEventListener("click", (event) => {
          this.close();
          this.onChooseSuggestion(item, event);
        });
      }
    }
    open() {
      super.open();
      this.inputEl.focus();
      this._refresh();
    }
    getSuggestions() {
      return [];
    }
    renderSuggestion(item, element) {
      element.setText(String(item));
    }
    onChooseSuggestion() {}
  }
  class FuzzySuggestModal extends SuggestModal {
    getSuggestions(query) {
      const words = query.toLowerCase().split(/\s+/).filter(Boolean);
      return this.getItems().filter((item) => words.every((w) => this.getItemText(item).toLowerCase().includes(w)));
    }
    renderSuggestion(item, element) {
      element.setText(this.getItemText(item));
    }
    onChooseSuggestion(item, event) {
      this.onChooseItem(item, event);
    }
    getItems() {
      return [];
    }
    getItemText(item) {
      return String(item);
    }
    onChooseItem() {}
  }

  class Notice {
    constructor(message, timeout) {
      this.noticeEl = document.createElement("div");
      this.setMessage(message, timeout);
    }
    setMessage(message, timeout) {
      const text = typeof message === "string" ? message : message && message.textContent ? message.textContent : "";
      send({ type: "notice", text, timeout: timeout === undefined ? 4000 : timeout });
      return this;
    }
    hide() {}
  }

  function normalizePath(path) {
    return String(path).replace(/\\/g, "/").replace(/\/+/g, "/").replace(/^\/|\/$/g, "").normalize("NFC");
  }

  function debounce(callback, wait = 0, resetTimer = false) {
    let timer = null;
    let latest = [];
    const run = (...args) => {
      latest = args;
      if (timer && !resetTimer) return run;
      clearTimeout(timer);
      timer = setTimeout(() => {
        timer = null;
        callback(...latest);
      }, wait);
      return run;
    };
    run.cancel = () => {
      clearTimeout(timer);
      timer = null;
      return run;
    };
    run.run = () => {
      if (timer) {
        clearTimeout(timer);
        timer = null;
        callback(...latest);
      }
    };
    return run;
  }

  async function requestUrl(request) {
    const options = typeof request === "string" ? { url: request } : request;
    const answer = await call("requestUrl", {
      url: options.url,
      method: options.method || "GET",
      headers: options.headers || {},
      body: typeof options.body === "string" ? options.body : options.body ? JSON.stringify(options.body) : null,
    });
    if (options.throw !== false && answer.status >= 400) throw new Error(`Request failed, status ${answer.status}`);
    return {
      status: answer.status,
      headers: answer.headers || {},
      text: answer.text,
      get json() {
        return JSON.parse(answer.text);
      },
      get arrayBuffer() {
        return new TextEncoder().encode(answer.text).buffer;
      },
    };
  }

  const Platform = {
    isDesktop: false,
    isMobile: false,
    isDesktopApp: false,
    isMobileApp: false,
    isIosApp: false,
    isAndroidApp: false,
    isMacOS: false,
    isWin: false,
    isLinux: false,
    isSafari: false,
    isPhone: false,
    isTablet: false,
  };

  const obsidian = {
    App: class {},
    Component,
    Events,
    ItemView,
    Plugin,
    PluginSettingTab,
    Setting,
    Modal,
    SuggestModal,
    Scope, EditorSuggest, PopoverSuggest, Menu, MenuItem,
    FuzzySuggestModal,
    Notice,
    TAbstractFile,
    TFile,
    TFolder,
    MarkdownView,
    editorInfoField: cm?.editorInfoField,
    editorLivePreviewField: cm?.editorLivePreviewField,
    MarkdownRenderChild,
    MarkdownRenderer,
    MarkdownPreviewRenderer,
    TextComponent,
    TextAreaComponent,
    ToggleComponent,
    DropdownComponent,
    SliderComponent,
    ButtonComponent,
    Platform,
    normalizePath,
    debounce,
    requestUrl,
    request: async (options) => (await requestUrl(options)).text,
    setIcon: (element) => element,
    addIcon: () => {},
    getAllTags: (cache) => (cache && cache.tags ? cache.tags.map((t) => t.tag) : []),
    parseFrontMatterAliases: (front) => {
      const aliases = front?.aliases ?? front?.alias;
      return aliases == null ? null : [].concat(aliases).filter(value => typeof value === "string");
    },
    parseFrontMatterTags: (front) => (front && front.tags ? [].concat(front.tags).map((t) => `#${t}`) : null),
    moment,
    apiVersion: "1.4.0",
  };

  /** Enough of moment.js for dates in notes: `format` with the usual tokens. */
  function moment(value) {
    const date = value === undefined ? new Date() : value === null ? new Date(NaN) : new Date(value);
    const pad = (n, width = 2) => String(n).padStart(width, "0");
    const wrapped = {
      _isAMomentObject: true,
      toDate: () => new Date(date),
      valueOf: () => date.getTime(),
      isValid: () => !Number.isNaN(date.getTime()),
      month: () => date.getMonth(),
      year: () => date.getFullYear(),
      weekday(day) { if (day === undefined) return date.getDay(); const next = new Date(date); next.setDate(next.getDate() + day - next.getDay()); return moment(next); },
      clone: () => moment(date),
      format(pattern = "YYYY-MM-DDTHH:mm:ssZ") {
        if (Number.isNaN(date.getTime())) return "Invalid date";
        const tokens = {
          YYYY: date.getFullYear(),
          YY: pad(date.getFullYear() % 100),
          MM: pad(date.getMonth() + 1),
          M: date.getMonth() + 1,
          DD: pad(date.getDate()),
          D: date.getDate(),
          HH: pad(date.getHours()),
          H: date.getHours(),
          mm: pad(date.getMinutes()),
          ss: pad(date.getSeconds()),
          Z: "",
        };
        return pattern.replace(/\[([^\]]*)\]|YYYY|YY|MM|M|DD|D|HH|H|mm|ss|Z/g, (token, literal) => (literal !== undefined ? literal : String(tokens[token])));
      },
      add(amount, unit) {
        const next = new Date(date);
        if (/^d/.test(unit)) next.setDate(next.getDate() + amount);
        else if (/^M|^mo/.test(unit)) next.setMonth(next.getMonth() + amount);
        else if (/^y/.test(unit)) next.setFullYear(next.getFullYear() + amount);
        else if (/^h/.test(unit)) next.setHours(next.getHours() + amount);
        else if (/^m/.test(unit)) next.setMinutes(next.getMinutes() + amount);
        return moment(next);
      },
      subtract(amount, unit) {
        return wrapped.add(-amount, unit);
      },
    };
    return wrapped;
  }

  moment.locale = () => "en";
  moment.localeData = () => ({ _week: { dow: 0 }, firstDayOfWeek: () => 0 });
  moment.weekdays = () => ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
  window.moment = moment;

  const unsupported = new Set();
  function partial(what) {
    if (unsupported.has(what)) return;
    unsupported.add(what);
    send({ type: "partial", what });
  }

  function report(error) {
    send({ type: "error", message: describe(error) });
  }

  // WebKit's stacks list frames only, without the message itself.
  function describe(error) {
    if (!error || !error.stack) return String(error);
    const stack = String(error.stack);
    return stack.includes(String(error.message)) ? stack : `${error.name || "Error"}: ${error.message}\n${stack}`;
  }
  window.addEventListener("error", (event) => report(event.error || event.message));
  window.addEventListener("unhandledrejection", (event) => report(event.reason));

  let plugin = null;
  let startData = null;

  // Real modules come from the app-resolved bundle. Older hosts without it
  // retain inert imports so commands, code blocks and suggestions still load.
  const EDITOR_MODULES = [
    "@codemirror/autocomplete", "@codemirror/commands", "@codemirror/language", "@codemirror/lint",
    "@codemirror/search", "@codemirror/state", "@codemirror/view",
    "@lezer/common", "@lezer/highlight", "@lezer/lr",
  ];
  function inert() {
    const target = function () {};
    const stand = new Proxy(target, {
      get(_, key) {
        if (key === "prototype") return target.prototype;
        if (key === Symbol.toPrimitive) return () => "";
        if (key === Symbol.iterator) return function* () {};
        // Bundlers check __esModule before taking a default export; "then"
        // must be absent, or awaiting a stand-in would never end.
        if (key === "__esModule") return true;
        if (key === "then") return undefined;
        return stand;
      },
      apply: () => stand,
      construct: () => stand,
    });
    return stand;
  }

  function setFiles(list, metadata) {
    files = list.map((f) => new TFile(f.path, f.stat));
    if (metadata) caches = metadata;
  }

  async function start(message) {
    Object.assign(Platform, message.platform || {});
    setFiles(message.files || [], message.metadata);
    startData = message.code.data || null;
    if (message.code.styles) {
      const style = document.createElement("style");
      style.textContent = message.code.styles;
      document.head.appendChild(style);
    }
    const module = { exports: {} };
    const require = (name) => {
      if (name === "obsidian") return obsidian;
      if (cm?.modules[name]) return cm.modules[name];
      if (EDITOR_MODULES.includes(name)) {
        partial("editor extensions");
        return inert();
      }
      throw new Error(`${name} is not available to plugins here`);
    };
    // The plugin's own code, as CommonJS, the way Obsidian loads it.
    new Function("require", "module", "exports", message.code.main)(require, module, module.exports);
    const Exported = module.exports.default || module.exports;
    plugin = new Exported(app, message.manifest);
    app.plugins.plugins[message.manifest.id] = plugin;
    await plugin.onload();
    send({ type: "ready" });
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
        case "files": {
          setFiles(message.files || [], message.metadata);
          const changed = message.changed ? vault.getAbstractFileByPath(message.changed) : null;
          if (changed) {
            vault.trigger("modify", changed);
            metadataCache.trigger("changed", changed, "", caches[changed.path] || null);
          }
          metadataCache.trigger("resolved");
          break;
        }
        case "command":
        case "command/run":
        case "command/check": {
          const prior = commandTail;
          let release;
          commandTail = new Promise((resolve) => { release = resolve; });
          await prior;
          const command = commands.get(message.id);
          if (!command) { release(); break; }
          const checking = message.type === "command/check";
          const context = makeEditor(message.editor || null, checking, true);
          const editor = context.facade;
          const view = { editor, file: message.editor ? new TFile(message.editor.path) : null };
          // The passed editor/view retain their source identity even across awaits.
          const previous = commandEditor;
          const previousFile = commandFile;
          const previousCheck = checkOnly;
          commandEditor = editor;
          commandFile = view.file;
          checkOnly = checking;
          let result = true;
          try {
            if (checking) {
              if (command.editorCheckCallback) result = Boolean(await command.editorCheckCallback(true, editor, view));
              else if (command.checkCallback) result = Boolean(await command.checkCallback(true));
              else result = !command.editorCallback || Boolean(message.editor);
              send({ type: "command/check", id: message.id, request: message.request, enabled: result });
            } else {
              if ((command.editorCallback || command.editorCheckCallback) && !message.editor) break;
              if (command.editorCallback) await command.editorCallback(editor, view);
              else if (command.editorCheckCallback) await command.editorCheckCallback(false, editor, view);
              else if (command.checkCallback) await command.checkCallback(false);
              else if (command.callback) await command.callback();
              context.submit();
            }
          } catch (error) {
            if (checking) send({ type: "command/check", id: message.id, request: message.request, enabled: false });
            throw error;
          } finally { commandEditor = previous; commandFile = previousFile; checkOnly = previousCheck; release(); }
          break;
        }
        case "ribbon":
          if (ribbon[message.index]) await ribbon[message.index](new MouseEvent("click"));
          break;
        case "render-code": {
          // Drawn here, in the plugin's own page; the app shows what came out.
          const element = document.createElement("div");
          const handler = codeProcessors.get(message.language);
          const context = {
            sourcePath: message.path,
            docId: message.path,
            frontmatter: (caches[message.path] || {}).frontmatter,
            // Renderers (Dataview's) draw when they are loaded, as in Obsidian.
            addChild: (child) => child && child.load && child.load(),
            getSectionInfo: () => null,
          };
          try {
            if (handler) await handler(message.source, element, context);
            await settled(element);
          } catch (error) {
            report(error);
          }
          send({ type: "rendered", id: message.id, html: element.innerHTML });
          break;
        }
        case "post-process": {
          // The note as the app drew it, changed here and sent back.
          const container = document.createElement("div");
          container.className = "markdown-preview-view markdown-rendered";
          container.innerHTML = message.html;
          await postProcess(container, message.path);
          await settled(container);
          send({ type: "rendered", id: message.id, html: container.innerHTML });
          break;
        }
        case "settings":
          if (!settingTab) break;
          settingTab.containerEl.empty();
          show(settingTab.containerEl, "settings");
          await settingTab.display();
          break;
        case "settings-close":
          if (settingTab) settingTab.hide();
          hideAll();
          break;
        case "suggest/query":
          await querySuggestions(message);
          break;
        case "suggest/choose":
          await chooseSuggestion(message);
          break;
        case "suggest/close":
          if (queuedSuggestQuery?.request === message.request) queuedSuggestQuery = null;
          if (suggestionSession?.request === message.request) suggestionSession.suggest.close();
          else if (message.request === latestSuggestRequest) {
            latestSuggestRequest = null;
            suggestEpoch++; queuedSuggestQuery = null;
            for (const s of editorSuggesters) { if (s !== selectingSuggest) { s.context = null; s.suggestions.values = []; } s.isOpen = false; }
          }
          break;
        case "menu/request":
          requestEditorMenu(message);
          break;
        case "menu/choose":
          await chooseEditorMenu(message);
          break;
        case "menu/close":
          editorMenus.delete(message.request);
          break;
        case "active":
          receiveSnapshot(message.editor || null);
          workspace.trigger("file-open", workspace.getActiveFile());
          workspace.trigger("active-leaf-change", markdownLeaf());
          break;
        case "editor/open":
        case "editor/snapshot":
        case "editor/update":
        case "editor/ack":
        case "editor/reject": {
          const changedFile = active?.path !== message.editor?.path || active?.session !== message.editor?.session;
          receiveSnapshot(message.editor || null);
          if (changedFile) { workspace.trigger("file-open", workspace.getActiveFile()); workspace.trigger("active-leaf-change", markdownLeaf()); }
          break;
        }
        case "editor/close":
          if (mirrorFor(message)) destroyMirror(mirrorFor(message));
          if (active && active.session === message.session && active.generation === message.generation) receiveSnapshot(null);
          break;
        case "event":
          if (message.name === "editor-change") {
            const context = makeEditor(message.editor, false, false);
            workspace.trigger("editor-change", context.facade, { editor: context.facade, file: new TFile(message.editor.path) });
          }
          break;
        case "stop":
          if (suggestionSession) suggestionSession.suggest.close();
          editorMenus.clear();
          for (const mirror of [...mirrors.values()]) destroyMirror(mirror);
          if (plugin) plugin.unload();
          plugin = null;
          break;
      }
    } catch (error) {
      report(error);
    }
  });

  send({ type: "loaded" });
})();
