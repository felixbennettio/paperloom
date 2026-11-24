#!/bin/bash
# Sourced by run.sh after the share test. Use only the phone's visible UI;
# missing WebView accessibility nodes leave evidence and a warning.

# Keep the mode separate from the dump path and any labels with spaces.
phone_probe() {
  local mode=$1; shift
  python3 "$WORK/screen.py" "phone-$mode" "$WORK/ui.xml" "$@"
}

phone_wait() {
  local until=$((SECONDS + 45))
  while [ "$SECONDS" -lt "$until" ]; do
    dump
    if phone_probe "$@" >/dev/null; then return 0; fi
    sleep 3
  done
  return 1
}

# WebView nodes sometimes disappear from a dump. Retry that known flake,
# but never repeat a tap that might already have created something.
phone_dump() {
  for _ in 1 2 3; do
    dump
    if phone_probe ready >/dev/null; then return 0; fi
    sleep 3
  done
  return 1
}

phone_focus() {
  adb shell dumpsys activity activities > "$WORK/activities.txt" 2>/dev/null || true
  sed -n '/topResumedActivity\|mResumedActivity/p' "$WORK/activities.txt" > "$WORK/focus.txt"
  if ! [ -s "$WORK/focus.txt" ]; then
    adb shell dumpsys window windows > "$WORK/windows.txt" 2>/dev/null || true
    sed -n '/mCurrentFocus=/p' "$WORK/windows.txt" > "$WORK/focus.txt"
  fi
  [ -s "$WORK/focus.txt" ] || return 2
  grep -Fq "$PACKAGE/" "$WORK/focus.txt"
}

phone_missing() {
  local seen
  seen=$(phone_probe text | tr '\n' ';') || seen="(WebView text unavailable)"
  warn "phone: $*; labels seen: $seen"
}

phone_tap() {
  local mode=$1 at; shift
  calm_system
  if ! phone_wait "$mode" "$@"; then return 1; fi
  at=$(phone_probe "$mode" "$@") || return 1
  [ -n "$at" ] || return 1
  # shellcheck disable=SC2086
  adb shell input tap $at || { fail "phone: tap $*"; return 1; }
  sleep 3
}

phone_key() {
  adb shell input keyevent "$1" || fail "phone: key $1"
  sleep 3
}

phone_hide_keyboard() {
  adb shell dumpsys input_method > "$WORK/keyboard.txt" 2>/dev/null || true
  if grep -Eq 'mInputShown=true|isInputViewShown=true' "$WORK/keyboard.txt"; then
    # Android consumes Back to hide a shown IME. Escape can reach the web
    # form's own dismissal handler and close the dialog before its shot.
    phone_key KEYCODE_BACK
  fi
}

phone_type() {
  adb shell input text "$1" || fail "phone: typing $1"
  sleep 3
}

phone_step=0
phone_shot() {
  local name count allowed
  calm_system
  sleep 4
  phone_step=$((phone_step + 1))
  name=$(printf 'phone-%02d-%s' "$phone_step" "$1")
  if [ "${2:-}" = background ]; then
    dump
  else
    phone_dump || warn "phone: $name has incomplete WebView nodes; review its screenshot"
  fi
  adb exec-out screencap -p > "$OUT/$name.png" || fail "phone: screenshot $name"
  cp "$WORK/ui.xml" "$OUT/$name.xml" || warn "phone: no XML for $name"
  python3 "$WORK/screen.py" text "$WORK/ui.xml" > "$OUT/$name.txt" || true
  {
    echo "--- $name"
    if ! phone_probe overflow; then echo "WebView or screen bounds unavailable (background shots have no app WebView)"; fi
  } > "$WORK/overflow.txt"
  cat "$WORK/overflow.txt" >> "$OUT/phone-overflow.txt"
  count=$(grep -c '^possible layout overflow:' "$WORK/overflow.txt") || count=0
  allowed=$(grep -c '^sideways row (allowed):' "$WORK/overflow.txt") || allowed=0
  [ "$count" -eq 0 ] || warn "phone: $name: $count possible overflow nodes; see phone-overflow.txt"
  [ "$allowed" -eq 0 ] || warn "phone: $name: $allowed allowed sideways-row nodes (Database source chips or HorizontalScrollView); see phone-overflow.txt"
  echo "--- $name"
  sed 's/^/    /' "$OUT/$name.txt"
}

# Check the dump kept with the screenshot. A complete tree with a missing
# control is a failure; an incomplete tree is left for a person to review.
phone_expect() {
  local description=$1; shift
  if phone_probe "$@" >/dev/null; then return 0; fi
  if phone_probe ready >/dev/null; then
    fail "phone: $description"
  else
    local focus_status=0
    phone_focus || focus_status=$?
    if [ "$focus_status" -eq 1 ]; then
      fail "phone: $description (app is not in the foreground)"
    else
      phone_missing "$description (incomplete WebView dump)"
    fi
  fi
  return 1
}

phone_capture() {
  local name=$1 description=$2; shift 2
  phone_wait "$@" || true
  phone_shot "$name"
  phone_expect "$description" "$@" || true
}

phone_tab() {
  phone_hide_keyboard
  if ! phone_tap nav "$1"; then
    phone_missing "bottom navigation $1"
    # If a complete dump lacks the bar, this is an obvious layout failure.
    if phone_probe ready >/dev/null; then fail "phone: bottom navigation $1"; fi
    return 1
  fi
}

phone_more() {
  phone_hide_keyboard
  if phone_tap nav More && phone_tap tap "$1"; then return 0; fi
  phone_missing "More → $1"
  if phone_probe ready >/dev/null; then fail "phone: More → $1"; fi
  return 1
}

phone_notes_menu() {
  # There are two More buttons: the first is Notes' narrow action menu;
  # the last is the bottom navigation menu.
  phone_hide_keyboard
  phone_tap tap More
}

phone_zoom() {
  local name=$1 used=0
  if phone_tap tap Fit; then used=1; fi
  if phone_tap tap "Zoom In"; then used=1; fi
  [ "$used" -eq 1 ] || phone_missing "$name: Fit / Zoom In unavailable"
  phone_shot "$name-zoom"
  if [ "$used" -eq 1 ]; then phone_expect "$name zoom controls" has "Zoom In" || true; fi
}

android_phone_test() {
  local label search_word focus_status=0
  echo "==> Phone layout and navigation"
  : > "$OUT/phone-overflow.txt"
  calm_system
  # The share dialogs are done; begin on the existing library, keeping data.
  adb shell am force-stop "$PACKAGE" || fail "phone: stopping before launch"
  adb shell monkey -p "$PACKAGE" -c android.intent.category.LAUNCHER 1 >/dev/null || fail "phone: launch"
  sleep 15
  phone_capture library "library list" has "All Items"
  for label in Library "Search Everything" Notes More; do
    phone_expect "bottom bar: $label" nav "$label" || true
  done
  search_word=$(phone_probe word) || {
    search_word=Paperloom
    phone_missing "cannot read an item title for Search; using the RIS record's word Paperloom"
  }

  echo "==> Phone: first library item and Android Back"
  if phone_tap item; then
    phone_capture item "item page" has "Reading Status"
    phone_expect "item back control (All Items)" back || true
    phone_key KEYCODE_BACK
    phone_capture item-back "Android Back returns to the library" has "Add Item"
  else
    phone_shot item-missing
    phone_expect "library has an item row from the share test" item || true
  fi

  echo "==> Phone: a PDF in the reader"
  # The newest item is the share test's (scanned) PDF; pdf.js must open it in
  # this WebView, which shows the page count only once it has.
  if phone_tap item; then
    adb shell input swipe 540 1700 540 700 500 || true
    sleep 2
    if phone_tap tap Read; then
      # The WebView reports "1 / 1" as three nodes; the "/" shows only with a PDF open.
      phone_capture pdf-reader "the reader opens the PDF (its page count shows)" has "/"
    else
      phone_shot pdf-read-missing
      phone_missing "Read button on the item page"
    fi
    phone_key KEYCODE_BACK
    phone_key KEYCODE_BACK
  fi

  echo "==> Phone: Search Everything"
  # Use a word from a visible title, even when an external share was skipped.
  if phone_tab "Search Everything"; then
    if phone_tap field "Words in titles, PDFs, annotations and notes"; then
      phone_type "$search_word"
      phone_key KEYCODE_ENTER
      phone_hide_keyboard
    else
      phone_missing "Search Everything field"
    fi
  fi
  # A populated input may expose only its value, losing the placeholder.
  phone_capture search "Search Everything field" field
  # uiautomator rarely reports what a WebView field holds: a warning only.
  phone_probe value "$search_word" >/dev/null || warn "phone: the typed search word is not in the dump; see the screenshot"

  echo "==> Phone: Online Search, Settings and Browser"
  phone_more "Online Search" || true
  phone_hide_keyboard
  phone_capture online-search "Online Search page" has "Database"
  phone_more Settings || true
  phone_capture settings "Settings page" has "Sync"
  # A swipe in the content, clear of the bottom navigation bar.
  adb shell input swipe 540 1800 540 650 600 || fail "phone: scroll Settings"
  sleep 4
  phone_shot settings-scrolled
  phone_expect "Settings content after scrolling" ready || true
  phone_more Browser || true
  phone_capture browser "Browser page" has "CNKI"

  echo "==> Phone: Notes and a mind map"
  phone_tab Notes || true
  phone_capture notes "Notes page" has "Quick Switcher"
  if phone_notes_menu && phone_tap tap "New Mind Map"; then
    phone_wait has "Zoom In" || true
    # New Mind Map creates immediately; rename through the existing Title field.
    if phone_tap field Title; then
      adb shell input keycombination 113 29 || fail "phone: select mind map title"
      phone_type PhoneMap
      phone_key KEYCODE_ENTER
      phone_hide_keyboard
    else
      phone_missing "mind map Title field; kept default name"
    fi
    phone_capture mind-map "Mind Map view" has "Zoom In"
    phone_zoom mind-map
  else
    phone_missing "New Mind Map entry point"
    phone_shot mind-map-unavailable
    phone_key KEYCODE_BACK
  fi
  phone_tab Notes || true
  phone_capture notes-after-map "Notes after mind map" has "Quick Switcher"

  echo "==> Phone: canvas"
  if phone_notes_menu && phone_tap tap "New Canvas"; then
    phone_capture canvas "Canvas view" has Canvas
    phone_zoom canvas
    phone_key KEYCODE_BACK
  else
    phone_missing "New Canvas entry point"
    phone_shot canvas-unavailable
    phone_key KEYCODE_BACK
  fi
  phone_tab Notes || true
  phone_capture notes-after-canvas "Notes after canvas" has "Quick Switcher"

  echo "==> Phone: diagram"
  if phone_notes_menu; then
    # This entry is normally desktop-only; inspect once instead of waiting
    # for a control the current phone layout does not expose.
    dump
    if phone_probe tap "New Diagram" >/dev/null && phone_tap tap "New Diagram"; then
      phone_capture diagram "Diagram view" has Diagram
      phone_zoom diagram
      phone_key KEYCODE_BACK
    else
      # The desktop New menu is hidden on phones. A normal note exposes
      # Insert Diagram in the narrow More menu; use that visible route.
      phone_missing "New Diagram unavailable; trying Quick Switcher → new note → Insert Diagram"
      phone_shot diagram-menu
      phone_key KEYCODE_BACK
      if phone_tap tap "Quick Switcher" && phone_tap field "Find or create a note…"; then
        phone_type PhoneDiagram
        phone_hide_keyboard
        if phone_tap tap 'Create “PhoneDiagram”'; then
          phone_wait field Title || true
          if phone_notes_menu && phone_tap tap "Insert Diagram"; then
            phone_capture diagram "Diagram view via Insert Diagram" has Diagram
            phone_zoom diagram
            phone_key KEYCODE_BACK
          else
            phone_missing "Insert Diagram entry point"
            phone_shot diagram-unavailable
            phone_key KEYCODE_BACK
          fi
        else
          phone_missing "Create “PhoneDiagram”"
          phone_shot diagram-note-unavailable
          phone_key KEYCODE_BACK
        fi
      else
        phone_missing "Quick Switcher entry point for diagram"
        phone_shot diagram-unavailable
        phone_key KEYCODE_BACK
      fi
    fi
  else
    phone_missing "Notes More menu for diagram"
    phone_shot diagram-unavailable
  fi
  phone_tab Notes || true
  phone_capture notes-after-diagram "Notes after diagram" has "Quick Switcher"

  echo "==> Phone: Graph"
  if phone_notes_menu && phone_tap tap Graph; then
    phone_capture graph "Graph view" has "Zoom In"
    phone_zoom graph
    phone_key KEYCODE_BACK
  else
    phone_missing "Graph entry point"
    phone_shot graph-unavailable
    phone_key KEYCODE_BACK
  fi
  phone_tab Notes || true
  phone_capture notes-after-graph "Notes after Graph" has "Quick Switcher"

  echo "==> Phone: Add Item and Android Back"
  phone_tab Library || true
  if phone_tap tap "Add Item"; then
    phone_hide_keyboard
    phone_capture add-item "Add Item dialog" has "Add by identifier"
    phone_key KEYCODE_BACK
    phone_capture add-item-back "library after closing Add Item" has "All Items"
    if phone_probe has "Add by identifier" >/dev/null; then fail "phone: Android Back did not close Add Item"; fi
  else
    phone_shot add-item-unavailable
    phone_expect "Add Item entry point" tap "Add Item" || true
  fi

  echo "==> Phone: Android Back at the library root"
  phone_tab Library || true
  phone_capture library-root "library root before Android Back" has "All Items"
  phone_key KEYCODE_BACK
  sleep 5
  phone_focus > /dev/null || focus_status=$?
  cp "$WORK/focus.txt" "$OUT/phone-root-back-focus.txt" || true
  phone_shot root-back background
  case "$focus_status" in
    0) warn "phone: Android Back at the library root left Paperloom in the foreground" ;;
    2) warn "phone: cannot determine the focused app after root Back; see phone-root-back-focus.txt" ;;
    *) echo "    Paperloom is no longer the focused app" ;;
  esac
  return 0
}
