import { describe, expect, it } from "vitest";
import { BIG_SPEND, isBigSpend } from "./kit";

describe("isBigSpend", () => {
  it("asks for $5,000 or more, or half the cash on hand", () => {
    expect(isBigSpend(BIG_SPEND, 1_000_000)).toBe(true);
    expect(isBigSpend(4_999, 1_000_000)).toBe(false);
    expect(isBigSpend(600, 1_000)).toBe(true);
    expect(isBigSpend(400, 1_000)).toBe(false);
    expect(isBigSpend(0, 0)).toBe(false);
  });
});
