// The script `harnex ask serve` sets into a decision page. The page carries
// markup only; `ASK` is defined ahead of this by the transport. It is driven
// through real browsers by crates/harness-cli/tests/ask-page.
//
// The page promises, for each ask:
// - one `fieldset[data-ask][data-version]`, its `data-ask` the ask's id and
//   its `data-version` the ask's version;
// - inside it, one `input[type="radio"]` named for the id per offered answer,
//   its `value` the answer's name;
// - inside it, for each answer whose note is not `none`, one
//   `[data-ask-note]` named for the answer, holding one `input` or `textarea`;
// and once, a `[data-ask-send]` whose content is what a page opened from disk
// shows. The script replaces that content with the send controls, so the
// page keeps its own elements outside it. Controls ship disabled; a page that
// breaks the promise says so there and sends nothing. A page that sets a
// Content-Security-Policy allows `script-src 'self'` and `connect-src 'self'`:
// this script is served from the page's origin and sends there.
//
// A page's own script, which runs before this one, hears three events on
// `document`. `ask-ready` fires once the controls are on, and `detail.asks` is
// the asks file. `ask-refused` fires each time a send is not taken and the
// controls stay on, once the status line says why: `detail.problem` is what it
// says, and `detail.asks` the ids of the asks it is about. Those are the ask
// whose answer or note was refused, or the asks a set asked together still
// waits on in the order the set names them, and none when the refusal is about
// the answers as a whole or the session was not reached. `ask-closed` fires
// once the controls are off for good, and `detail.answered` says whether the
// answers were taken. The page's script answers by checking a radio and
// letting its `change` bubble, and sends by clicking `.ask-send`. Marking,
// focusing or scrolling to what a refusal names is the page's.
//
// A radio the page ships checked is an answer the person already gave, such
// as one `ask current` finds `current` or `incomplete`: it counts, and is
// sent with its note as the page filled that field, unless they choose
// another. A radio cannot be cleared, so a page checks none it was not given.
// A set asked together that the page checks in part is sent once the person
// chooses the rest.
//
// Pages are written against what this header names, so a change to it is a
// break (.claude/rules/making-changes.md).
(() => {
  "use strict";
  const { endpoint, asks, words, deadline } = ASK;
  const fill = (template, values) =>
    template.replace(/\{(\w+)\}/g, (whole, name) => (name in values ? String(values[name]) : whole));
  const element = (tag, className) => {
    const made = document.createElement(tag);
    made.className = className;
    return made;
  };

  const slot = document.querySelector("[data-ask-send]");
  const bar = slot ?? document.body.appendChild(document.createElement("div"));
  const progress = element("p", "ask-progress");
  const button = element("button", "ask-send");
  const until = element("p", "ask-until");
  const status = element("p", "ask-status");
  button.type = "button";
  button.textContent = words.send;
  status.setAttribute("role", "status");
  const closes = new Date(deadline);
  const twoDigits = (n) => String(n).padStart(2, "0");
  until.textContent = fill(words.until, {
    time:
      `${closes.getFullYear()}-${twoDigits(closes.getMonth() + 1)}-${twoDigits(closes.getDate())} ` +
      `${twoDigits(closes.getHours())}:${twoDigits(closes.getMinutes())}`,
  });
  bar.replaceChildren(progress, button, until, status);
  bar.hidden = false;
  const say = (text) => {
    status.textContent = text;
  };

  const fields = [...document.querySelectorAll("fieldset[data-ask]")];
  const fieldOf = (ask) => fields.find((field) => field.dataset.ask === ask.id);
  const radiosOf = (ask) =>
    [...fieldOf(ask).querySelectorAll('input[type="radio"]')].filter((radio) => radio.name === ask.id);
  const noteOf = (ask, name) =>
    [...fieldOf(ask).querySelectorAll("[data-ask-note]")].find((note) => note.dataset.askNote === name);
  const inputOf = (ask, name) => noteOf(ask, name)?.querySelector("input, textarea") ?? null;

  const problems = [];
  if (slot === null) problems.push(words.page_no_send);
  for (const ask of asks.asks) {
    const label = ask.label;
    if (fieldOf(ask) === undefined) {
      problems.push(fill(words.page_missing, { label }));
      continue;
    }
    if (fieldOf(ask).dataset.version !== ask.version) {
      problems.push(fill(words.page_version_differs, { label }));
    }
    const values = radiosOf(ask).map((radio) => radio.value);
    if (values.length !== ask.answers.length || ask.answers.some((offered) => !values.includes(offered.name))) {
      problems.push(fill(words.page_answers_differ, { label }));
    }
    for (const offered of ask.answers) {
      if (offered.note !== "none" && inputOf(ask, offered.name) === null) {
        problems.push(fill(words.page_note_missing, { label, answer: offered.name }));
      }
    }
  }
  for (const field of fields) {
    if (!asks.asks.some((ask) => ask.id === field.dataset.ask)) {
      problems.push(fill(words.page_unasked, { id: field.dataset.ask }));
    }
  }
  if (problems.length > 0) {
    button.disabled = true;
    say(problems.join(" "));
    return;
  }

  const controls = asks.asks.flatMap((ask) => [...fieldOf(ask).querySelectorAll("input, textarea")]);
  const chosen = (ask) => radiosOf(ask).find((radio) => radio.checked);
  const refresh = () => {
    let answered = 0;
    for (const ask of asks.asks) {
      const pick = chosen(ask);
      if (pick !== undefined) answered += 1;
      for (const offered of ask.answers) {
        const note = noteOf(ask, offered.name);
        if (note !== undefined) note.hidden = pick?.value !== offered.name;
      }
    }
    progress.textContent = fill(words.progress, { answered, asked: asks.asks.length });
  };
  for (const control of controls) control.disabled = false;
  for (const ask of asks.asks) fieldOf(ask).addEventListener("change", refresh);
  refresh();
  document.dispatchEvent(new CustomEvent("ask-ready", { detail: { asks } }));

  let done = false;
  const finish = (text, answered) => {
    done = true;
    for (const control of controls) control.disabled = true;
    button.disabled = true;
    say(text);
    document.dispatchEvent(new CustomEvent("ask-closed", { detail: { answered } }));
  };
  const refused = (problem, named) => {
    button.disabled = false;
    say(problem);
    document.dispatchEvent(new CustomEvent("ask-refused", { detail: { problem, asks: named } }));
  };
  button.addEventListener("click", async () => {
    if (done) return;
    const answers = [];
    for (const ask of asks.asks) {
      const pick = chosen(ask);
      if (pick === undefined) continue;
      const input = inputOf(ask, pick.value);
      answers.push({ id: ask.id, answer: pick.value, note: input === null ? null : input.value });
    }
    button.disabled = true;
    let response;
    try {
      response = await fetch(endpoint, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ answers }),
      });
    } catch {
      refused(words.unreachable, []);
      return;
    }
    // A refusal the transport writes carries `problem`, `final` and `asks`;
    // one it does not, such as a 403 to a request it will not read, has no
    // body.
    const reply = await response.json().catch(() => ({}));
    if (response.ok) {
      finish(words.sent, true);
    } else if (reply.final === true) {
      finish(reply.problem ?? words.unreadable, false);
    } else {
      refused(reply.problem ?? words.unreadable, reply.asks ?? []);
    }
  });
})();
