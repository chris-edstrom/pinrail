import { expect, test } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { reviewFrom, mountPlugin } from "pinrail-sdk/testing";
import { scratch } from "./scratch.cjs";

// The comment field the stylesheet draws, and Pinrail.fitField, which sizes it.

/** A plugin whose view is a field with a cancel and an add button, empty: the
 *  stylesheet draws their icons. */
function composer(): string {
  const dir = scratch("pinrail-compose-");
  fs.mkdirSync(path.join(dir, "view"), { recursive: true });
  fs.writeFileSync(path.join(dir, "manifest.json"), JSON.stringify({ name: "compose", version: "1.0.0" }));
  const field = (id: string) => `
<div class="pinrail-compose" style="width: 360px">
  <textarea class="pinrail-compose-input" id="${id}" rows="1" aria-label="${id}"></textarea>
  <span class="pinrail-compose-keys">Enter to add</span>
  <span class="pinrail-compose-tools">
    <button type="button" class="pinrail-compose-icon pinrail-compose-cancel" id="${id}-cancel" aria-label="Cancel"></button>
    <button type="button" class="pinrail-compose-icon pinrail-compose-send" id="${id}-send" aria-label="Add"></button>
  </span>
</div>
<div class="saved pinrail-comment" id="${id}-saved">
  <span>Your comment</span>
  <span class="pinrail-comment-actions">
    <button type="button" class="pinrail-comment-edit" id="${id}-edit" aria-label="Edit"></button>
    <button type="button" class="pinrail-comment-delete" id="${id}-delete" aria-label="Delete"></button>
  </span>
</div>`;
  fs.writeFileSync(
    path.join(dir, "view", "index.html"),
    `<!doctype html>
<meta charset="utf-8">
<link rel="stylesheet" href="/sdk/v1/pinrail-plugin.css">
<script src="/sdk/v1/pinrail-plugin.js"></script>
${field("comment")}
<script>
  for (const el of document.querySelectorAll("textarea")) {
    Pinrail.fitField(el);
    el.addEventListener("input", () => Pinrail.fitField(el));
  }
  Pinrail.connect({ onInit() {} });
</script>`,
  );
  return dir;
}

test("fitField grows the field and goes wide once the text wraps or has a new line", async ({ page }) => {
  const plugin = await mountPlugin(page, composer(), { review: reviewFrom({ title: "Compose", payload: {} }) });
  const f = plugin.frame;
  const box = f.locator(".pinrail-compose").first();
  const comment = f.locator("#comment");

  // empty: one line, the keys hidden
  await expect(box).not.toHaveClass(/pinrail-compose-wide/);
  await expect(box.locator(".pinrail-compose-keys")).toBeHidden();
  const oneLine = (await comment.boundingBox())!.height;

  await comment.fill("Short");
  await expect(box).not.toHaveClass(/pinrail-compose-wide/);

  // long enough to wrap: wide, taller, and the keys on show
  await comment.fill("A comment long enough to wrap onto a second line in a field this narrow, and then some more.");
  await expect(box).toHaveClass(/pinrail-compose-wide/);
  await expect(box.locator(".pinrail-compose-keys")).toBeVisible();
  expect((await comment.boundingBox())!.height).toBeGreaterThan(oneLine * 1.5);

  // emptying it takes the wide layout back, and a new line makes it wide
  await comment.fill("");
  await expect(box).not.toHaveClass(/pinrail-compose-wide/);
  await comment.fill("one\ntwo");
  await expect(box).toHaveClass(/pinrail-compose-wide/);

  // the add button is the view's: fitField leaves it as it is
  await expect(f.locator("#comment-send")).toBeEnabled();
  await comment.fill("");
  await expect(f.locator("#comment-send")).toBeEnabled();
});

test("the stylesheet draws the icons of a field's buttons and of a comment's edit and delete", async ({ page }) => {
  const plugin = await mountPlugin(page, composer(), { review: reviewFrom({ title: "Compose", payload: {} }) });
  const f = plugin.frame;
  for (const id of ["#comment-send", "#comment-cancel", "#comment-edit", "#comment-delete"]) {
    const icon = await f.locator(id).evaluate((el) => {
      const s = getComputedStyle(el, "::before");
      return { mask: s.webkitMaskImage || s.maskImage, width: parseFloat(s.width) };
    });
    expect(icon.mask, `${id} has no icon`).toContain("data:image/svg+xml");
    expect(icon.width).toBeGreaterThan(8);
  }
});

test("a comment's edit and delete show while the pointer is over it, or the focus is in it", async ({ page }) => {
  const plugin = await mountPlugin(page, composer(), { review: reviewFrom({ title: "Compose", payload: {} }) });
  const f = plugin.frame;
  const opacity = () => f.locator("#comment-edit").evaluate((el) => getComputedStyle(el.parentElement!).opacity);
  await f.locator("#comment").hover();
  await expect.poll(opacity).toBe("0");
  await f.locator("#comment-saved").hover();
  await expect.poll(opacity).toBe("1");
  await f.locator("#comment").hover();
  await f.locator("#comment-edit").focus();
  await expect.poll(opacity).toBe("1");
});
