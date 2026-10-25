// TEMPORARY measurement harness (deleted after use).
// Loads the BUILT extension in Chromium, opens the dashboard, and reports the
// exact geometry of the Publishing Defaults rows and the Variables labels.
import { chromium } from "@playwright/test";
import { existsSync, mkdtempSync } from "fs";
import os from "os";
import path from "path";

const extensionPath = path.resolve(".output/chrome-mv3");
const systemChromium = "/run/current-system/sw/bin/chromium";

const userDataDir = mkdtempSync(path.join(os.tmpdir(), "vidpro-measure-"));
const executablePath = existsSync(systemChromium) ? systemChromium : undefined;

const context = await chromium.launchPersistentContext(userDataDir, {
  executablePath,
  headless: true,
  viewport: { width: 1920, height: 1080 },
  args: [
    `--disable-extensions-except=${extensionPath}`,
    `--load-extension=${extensionPath}`,
    "--no-sandbox",
  ],
});

let [background] = context.serviceWorkers();
if (!background) background = await context.waitForEvent("serviceworker", { timeout: 30000 });
const extensionId = background.url().split("/")[2];
console.log("extensionId", extensionId);

const page = await context.newPage();
await page.goto(`chrome-extension://${extensionId}/dashboard.html?tab=settings`);
await page.waitForLoadState("networkidle");
await page.waitForTimeout(1500);

const data = await page.evaluate(() => {
  const r = (el) => {
    if (!el) return null;
    const b = el.getBoundingClientRect();
    return {
      x: +b.x.toFixed(1),
      y: +b.y.toFixed(1),
      w: +b.width.toFixed(1),
      h: +b.height.toFixed(1),
      right: +b.right.toFixed(1),
      bottom: +b.bottom.toFixed(1),
      cy: +(b.y + b.height / 2).toFixed(1),
    };
  };
  const out = {};

  const kids = document.querySelector('[data-testid="kids-state"]');
  const kidsLabel = kids?.closest("label");
  out.madeForKids = {
    label: r(kidsLabel),
    toggle: r(kidsLabel?.querySelector("div.w-11")),
    text: r(kidsLabel?.querySelector("span.text-xs")),
    state: r(kids),
    gridCell: r(kidsLabel?.parentElement),
  };

  const paid = document.querySelector('[data-testid="paid-state"]');
  const paidLabel = paid?.closest("label");
  out.paid = {
    label: r(paidLabel),
    toggle: r(paidLabel?.querySelector("div.w-11")),
    text: r(paidLabel?.querySelector("span.text-xs")),
    state: r(paid),
  };

  // The Language input and the Category select, for column comparison.
  const langLabel = Array.from(document.querySelectorAll("label")).find((l) =>
    l.textContent?.trim().startsWith("Language")
  );
  out.language = {
    label: r(langLabel),
    input: r(langLabel?.parentElement?.querySelector("input")),
  };
  const catLabel = Array.from(document.querySelectorAll("label")).find((l) =>
    l.textContent?.trim().startsWith("Category")
  );
  out.category = {
    label: r(catLabel),
    select: r(catLabel?.parentElement?.querySelector("select")),
  };
  const altered = Array.from(document.querySelectorAll("div")).find(
    (d) => d.textContent?.trim().startsWith("Altered content") && d.className.includes("rounded-lg")
  );
  out.altered = { card: r(altered) };

  // Grids: parent grid of each of the two rows.
  out.kidsGrid = r(kidsLabel?.closest(".grid"));
  out.alteredGrid = r(altered?.closest(".grid"));

  // Variable labels: name text vs token pill.
  out.variables = Array.from(document.querySelectorAll('label[for^="thumbnail-var-"]')).map((l) => {
    const pill = l.querySelector("span");
    const nameNode = l.firstChild;
    const nameRange = document.createRange();
    nameRange.selectNodeContents(l);
    // Measure the name text only: the label's first child is the text node.
    const nameRect = (() => {
      const range = document.createRange();
      range.setStart(nameNode, 0);
      range.setEnd(nameNode, nameNode.length ?? 0);
      const b = range.getBoundingClientRect();
      return { x: +b.x.toFixed(1), right: +b.right.toFixed(1), w: +b.width.toFixed(1), cy: +(b.y + b.height / 2).toFixed(1) };
    })();
    const pr = pill?.getBoundingClientRect();
    return {
      for: l.getAttribute("for"),
      text: l.textContent?.trim(),
      name: nameRect,
      pill: pr
        ? {
            x: +pr.x.toFixed(1),
            w: +pr.width.toFixed(1),
            h: +pr.height.toFixed(1),
            cy: +(pr.y + pr.height / 2).toFixed(1),
          }
        : null,
      gapPx: pr ? +(pr.x - nameRect.right).toFixed(1) : null,
      label: r(l),
      input: r(document.getElementById(l.getAttribute("for"))),
      pillClass: pill?.className,
    };
  });

  // Library chip row.
  const chipRow = document.querySelector('[aria-label="Variable library"]');
  out.chipRow = { rect: r(chipRow), chips: Array.from(chipRow?.children ?? []).map(r) };

  // Sidebar bottom.
  const sidebar = document.querySelector("aside[role='navigation']");
  out.sidebar = {
    rect: r(sidebar),
    bottomZone: r(sidebar?.lastElementChild),
    html: sidebar?.lastElementChild?.innerHTML?.slice(0, 400),
  };
  const support = Array.from(document.querySelectorAll("a")).find((a) =>
    a.textContent?.includes("Support VidPro")
  );
  out.support = { rect: r(support), parentRect: r(support?.parentElement) };
  const status = Array.from(document.querySelectorAll("span")).find((s) => s.textContent?.trim() === "AI Ready");
  out.status = { rect: r(status), parentRect: r(status?.parentElement?.parentElement) };

  return out;
});

console.log(JSON.stringify(data, null, 2));

await page.screenshot({ path: ".pi/measure-dash-full.png", fullPage: true });

const clip = await page.evaluate(() => {
  const h = Array.from(document.querySelectorAll("h3,h4,h2")).find((x) =>
    x.textContent?.trim().startsWith("Publishing Defaults")
  );
  const b = h?.getBoundingClientRect();
  return b ? { x: 0, y: Math.max(0, b.y - 30), width: window.innerWidth, height: 700 } : null;
});
if (clip) await page.screenshot({ path: ".pi/measure-dash-pub.png", clip });

await context.close();
