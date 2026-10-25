# Final narrow review supplement

## Actual historical measurement lint repair
```diff
diff --git a/extensions/standalones/vidpro-extension/scripts/measure-dashboard.mjs b/extensions/standalones/vidpro-extension/scripts/measure-dashboard.mjs
index 019afc24f6..95f1415aab 100644
--- a/extensions/standalones/vidpro-extension/scripts/measure-dashboard.mjs
+++ b/extensions/standalones/vidpro-extension/scripts/measure-dashboard.mjs
@@ -101,8 +101,8 @@ const data = await page.evaluate(() => {
     // Measure the name text only: the label's first child is the text node.
     const nameRect = (() => {
       const range = document.createRange();
-      range.setStart(l.firstChild, 0);
-      range.setEnd(l.firstChild, l.firstChild.length ?? 0);
+      range.setStart(nameNode, 0);
+      range.setEnd(nameNode, nameNode.length ?? 0);
       const b = range.getBoundingClientRect();
       return { x: +b.x.toFixed(1), right: +b.right.toFixed(1), w: +b.width.toFixed(1), cy: +(b.y + b.height / 2).toFixed(1) };
     })();
@@ -156,7 +156,7 @@ const clip = await page.evaluate(() => {
     x.textContent?.trim().startsWith("Publishing Defaults")
   );
   const b = h?.getBoundingClientRect();
-  return b ? { x: 0, y: Math.max(0, b.y - 30), width: innerWidth, height: 700 } : null;
+  return b ? { x: 0, y: Math.max(0, b.y - 30), width: window.innerWidth, height: 700 } : null;
 });
 if (clip) await page.screenshot({ path: ".pi/measure-dash-pub.png", clip });
 

```

## Eligible-surface negative fixtures (HEAD to current worktree; not yet claiming autonomous commit)
```diff
diff --git a/extensions/standalones/youtube-filter/tests/rules.test.ts b/extensions/standalones/youtube-filter/tests/rules.test.ts
index 903faacf95..e79d56a134 100644
--- a/extensions/standalones/youtube-filter/tests/rules.test.ts
+++ b/extensions/standalones/youtube-filter/tests/rules.test.ts
@@ -368,8 +368,8 @@ describe('the watched rule (hideWatchedVideos)', () => {
 
   it('is a no-op while the flag is off', () => {
     const s = settings({ rules: { ...DEFAULT_SETTINGS.rules, hideWatchedVideos: false } });
-    expect(watchedReason(card({ watched: true }), s)).toBeNull();
-    expect(evaluateRules(card({ watched: true }), s, null).hide).toBe(false);
+    expect(watchedReason(card({ watched: true, surface: 'subscriptions' }), s)).toBeNull();
+    expect(evaluateRules(card({ watched: true, surface: 'subscriptions' }), s, null).hide).toBe(false);
   });
 
   it('hides a card YouTube marks as watched, with a reason', () => {
@@ -394,11 +394,11 @@ describe('the watched rule (hideWatchedVideos)', () => {
 
   it('leaves an unwatched card alone', () => {
     const s = watched();
-    expect(watchedReason(card({ watched: false }), s)).toBeNull();
+    expect(watchedReason(card({ watched: false, surface: 'subscriptions' }), s)).toBeNull();
     // A card whose signal we could not read is NOT treated as watched:
     // the failure mode is a card that stays, never one that wrongly goes.
-    expect(watchedReason(card({}), s)).toBeNull();
-    expect(evaluateRules(card({}), s, null).hide).toBe(false);
+    expect(watchedReason(card({ surface: 'subscriptions' }), s)).toBeNull();
+    expect(evaluateRules(card({ surface: 'subscriptions' }), s, null).hide).toBe(false);
   });
 
   it('an allowed channel still wins over the watched rule', () => {

```

## Browser two-column separation (55fb4b081a4ace747bc8021d12e6fa89bdb6641e)
```diff
diff --git a/extensions/standalones/vidpro-extension/e2e/publishing-row-layout.spec.ts b/extensions/standalones/vidpro-extension/e2e/publishing-row-layout.spec.ts
index eea85012b0..a98e070e9c 100644
--- a/extensions/standalones/vidpro-extension/e2e/publishing-row-layout.spec.ts
+++ b/extensions/standalones/vidpro-extension/e2e/publishing-row-layout.spec.ts
@@ -19,6 +19,7 @@ test("publishing control edges align and an actual vertical offset is detected",
     const difference = async () => {
       const [select, box] = await Promise.all([category.boundingBox(), kidsBox.boundingBox()]);
       if (!select || !box) throw new Error("publishing control geometry unavailable");
+      expect(box.x).toBeGreaterThanOrEqual(select.x + select.width - 1);
       return Math.abs(select.y - box.y);
     };
     // A desktop two-column row must share a control top edge. One CSS pixel

```

Watched-rule suite:42passed,0failed. Eligible subscriptions make OFF/unwatched checks non-vacuous. No production policy change. Actual renderer geometry1passed, including harmless class and24px offset negative plus horizontal separation. Historical lint repair was later superseded by owner measurement changes/deletion.
