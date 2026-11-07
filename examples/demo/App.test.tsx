// The shell's routes, driven as a user drives them: home lists the products, a row opens its screen, back returns.
import { expect, open, test } from "nsh/test";

const PRODUCT_ROUTES = ["auth", "database", "storage", "messaging", "analytics", "monitor"] as const;

test("home lists every product route", async () => {
  const app = await open("./App.tsx", { clock: {} });
  for (const route of PRODUCT_ROUTES) {
    expect(app.count(`id:row-${route}`)).toBe(1);
  }
});

test("a product row opens its screen and back returns home", async () => {
  const app = await open("./App.tsx", { clock: {} });
  await app.press("id:row-auth");
  await app.waitFor("id:page-auth");
  await app.waitFor("text:Email");
  await app.back();
  await app.waitFor("id:page-home");
});

test("every product route pushes and pops, the monitor included", async () => {
  const app = await open("./App.tsx", { clock: {} });
  for (const route of PRODUCT_ROUTES) {
    await app.press(`id:row-${route}`);
    await app.waitFor(`id:page-${route}`);
    await app.back();
    await app.waitFor("id:page-home");
  }
});
