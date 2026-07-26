import { describe, expect, it } from "vitest";
import { formatBeijingDateTimeSeconds } from "./time-format";

describe("Beijing date time formatting", () => {
  it("uses Asia/Shanghai independently of the host timezone", () => {
    const formatted = formatBeijingDateTimeSeconds(1_783_890_765, "zh-CN");
    expect(formatted).toContain("2026");
    expect(formatted).toContain("7月13日");
    expect(formatted).toContain("05:12");
  });
});
