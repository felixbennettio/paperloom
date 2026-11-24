// Reads the page in the active tab with Paperloom's capture script and
// sends what it finds to the app, which runs a small server on this
// computer (127.0.0.1) for the extension.
"use strict";

const APP = "http://127.0.0.1:23121";
const api = globalThis.browser ?? globalThis.chrome;
const say = (key, ...values) => api.i18n.getMessage(key, values.map(String)) || key;
const status = document.getElementById("status");
const works = document.getElementById("works");
const save = document.getElementById("save");

for (const element of document.querySelectorAll("[data-message]")) {
  element.textContent = say(element.dataset.message);
}

async function call(path, body) {
  const response = await fetch(APP + path, {
    method: body === undefined ? "GET" : "POST",
    headers: { "Content-Type": "application/json" },
    body,
  });
  const answer = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(answer.error || response.statusText);
  return answer;
}

function show(page) {
  works.replaceChildren();
  for (const hit of page.hits) {
    const item = document.createElement("li");
    const label = document.createElement("label");
    const box = document.createElement("input");
    box.type = "checkbox";
    box.value = hit.key;
    box.checked = page.hits.length === 1 || !hit.libraryItemId;
    box.disabled = Boolean(hit.libraryItemId);
    const text = document.createElement("span");
    text.textContent = hit.detail.title;
    const details = document.createElement("small");
    const authors = hit.detail.authors.slice(0, 3).join(", ") + (hit.detail.authors.length > 3 ? " …" : "");
    details.textContent = hit.libraryItemId
      ? say("already")
      : [authors, hit.detail.containerTitle, hit.detail.year].filter(Boolean).join(" · ");
    text.append(details);
    label.append(box, text);
    item.append(label);
    works.append(item);
  }
  const chosen = () => [...works.querySelectorAll("input:checked:not(:disabled)")].map((box) => box.value);
  save.hidden = page.hits.length === 0;
  save.disabled = chosen().length === 0;
  works.onchange = () => (save.disabled = chosen().length === 0);
  save.onclick = async () => {
    save.disabled = true;
    try {
      const report = await call("/save", JSON.stringify({ keys: chosen() }));
      status.textContent = say("saved", report.added, report.duplicates);
      works.replaceChildren();
      save.hidden = true;
    } catch (error) {
      status.textContent = error.message;
      save.disabled = false;
    }
  };
}

async function start() {
  try {
    await call("/ping");
  } catch {
    status.textContent = say("noApp");
    return;
  }
  status.textContent = say("reading");
  try {
    const [tab] = await api.tabs.query({ active: true, currentWindow: true });
    const [result] = await api.scripting.executeScript({ target: { tabId: tab.id }, files: ["capture.js"] });
    const page = await call("/read", result.result);
    status.textContent = page.hits.length ? say("found", page.hits.length) : say("nothing");
    show(page);
  } catch (error) {
    status.textContent = say("cannotRead") + " " + (error.message || "");
  }
}

start();
