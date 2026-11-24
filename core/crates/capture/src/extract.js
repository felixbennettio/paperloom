// Reads the page it runs in: its embedded metadata (meta tags, COinS,
// JSON-LD) and, on a site the rules know, the works it lists or shows.
// Called with the rules (core/rules/sites.json); answers a JSON string for
// paperloom-capture to turn into items. Reads only; changes nothing.
(function (rules) {
  "use strict";

  var MOST_RECORDS = 200;

  function clean(text) {
    return (text || "").replace(/\s+/g, " ").trim();
  }

  function textOf(element) {
    var shown = element.innerText;
    return clean(typeof shown === "string" && shown.trim() !== "" ? shown : element.textContent);
  }

  // A selector, or a list of them tried in turn until one finds something.
  function elements(root, selector) {
    var choices = Array.isArray(selector) ? selector : [selector];
    for (var i = 0; i < choices.length; i++) {
      try {
        var found = root.querySelectorAll(choices[i]);
        if (found.length) return Array.prototype.slice.call(found);
      } catch (e) {
        // A selector this browser does not know; try the next.
      }
    }
    return [];
  }

  function first(root, selector) {
    var found = elements(root, selector);
    return found.length ? found[0] : null;
  }

  function valueOf(element, attribute) {
    if (!attribute) return textOf(element);
    if (attribute === "href" || attribute === "src") {
      return clean(element[attribute] || element.getAttribute(attribute));
    }
    return clean(element.getAttribute(attribute));
  }

  // { sel, attr, all, drop, match }: the text (or attribute) of what `sel`
  // finds under `root`, without values `drop` matches, cut to what `match`
  // (or its first group) matches; every value when `all`.
  function pick(root, spec) {
    var found = spec.sel ? elements(root, spec.sel) : [root];
    var values = found.map(function (element) {
      return valueOf(element, spec.attr);
    });
    if (spec.drop) {
      var drop = new RegExp(spec.drop);
      values = values.filter(function (value) {
        return !drop.test(value);
      });
    }
    if (spec.match) {
      var match = new RegExp(spec.match);
      values = values.map(function (value) {
        var m = match.exec(value);
        return m ? clean(m.length > 1 && m[1] !== undefined ? m[1] : m[0]) : "";
      });
    }
    values = values.filter(Boolean);
    if (!values.length) return null;
    return spec.all ? values : values[0];
  }

  function record(root, page) {
    var out = {};
    var fields = page.fields || {};
    Object.keys(fields).forEach(function (name) {
      var value = pick(root, fields[name]);
      if (value !== null) out[name] = value;
    });
    var labels = page.labels;
    if (labels) {
      elements(root, labels.rows).forEach(function (row) {
        var labelElement = row.querySelector(labels.label);
        if (!labelElement) return;
        var label = (labelElement.textContent || "").replace(/\s+/g, "").replace(/[:：]+$/, "");
        var spec = labels.map[label];
        if (!spec) return;
        if (typeof spec === "string") spec = { field: spec };
        var valueElement = labels.value ? first(row, labels.value) : row;
        if (!valueElement || valueElement === labelElement) return;
        var value = pick(valueElement, spec);
        if (value !== null && !(spec.field in out)) out[spec.field] = value;
      });
    }
    return out;
  }

  function siteHere() {
    var host = location.hostname;
    for (var i = 0; i < rules.sites.length; i++) {
      var site = rules.sites[i];
      var known = site.hosts.some(function (h) {
        return host === h || host.slice(-h.length - 1) === "." + h;
      });
      if (known) return site;
    }
    return null;
  }

  function pageHere(site) {
    for (var i = 0; i < site.pages.length; i++) {
      var page = site.pages[i];
      if (page.url && !new RegExp(page.url).test(location.href)) continue;
      if (page.records) {
        if (elements(document, page.records).length) return page;
        continue;
      }
      if (page.when && !elements(document, page.when).length) continue;
      return page;
    }
    return null;
  }

  var site = siteHere();
  var page = site ? pageHere(site) : null;
  var records = [];
  if (page && page.records) {
    records = elements(document, page.records)
      .slice(0, MOST_RECORDS)
      .map(function (element) {
        return record(element, page);
      });
  } else if (page) {
    records = [record(document, page)];
  }
  records = records.filter(function (r) {
    return Object.keys(r).length > 0;
  });

  var meta = Array.prototype.slice
    .call(document.querySelectorAll("meta[name], meta[property]"))
    .map(function (m) {
      return [m.getAttribute("name") || m.getAttribute("property"), clean(m.getAttribute("content"))];
    })
    .filter(function (pair) {
      return pair[0] && pair[1];
    })
    .slice(0, 2000);
  var coins = Array.prototype.slice
    .call(document.querySelectorAll("span.Z3988"))
    .map(function (span) {
      return span.getAttribute("title") || "";
    })
    .filter(Boolean)
    .slice(0, MOST_RECORDS);
  var jsonLd = Array.prototype.slice
    .call(document.querySelectorAll('script[type="application/ld+json"]'))
    .map(function (script) {
      return script.textContent || "";
    })
    .filter(function (text) {
      return text.length > 0 && text.length < 500000;
    })
    .slice(0, 20);

  return JSON.stringify({
    url: location.href,
    title: clean(document.title),
    lang: document.documentElement.lang || null,
    site: site ? site.id : null,
    page: page ? page.kind : null,
    meta: meta,
    coins: coins,
    jsonLd: jsonLd,
    records: records,
  });
})
