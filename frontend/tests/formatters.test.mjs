import { test, describe } from "node:test";
import assert from "node:assert/strict";
import {
  formatPaise,
  amountToWords,
  computeTaxSlabs,
  calculateItemSubtotalPaise,
  calculateLineTaxPaise,
} from "../lib/api.ts";

describe("formatPaise - Indian Currency Formatter", () => {
  test("formats zero correctly", () => {
    assert.equal(formatPaise(0), "₹0.00");
  });

  test("formats fractional paise under 1 rupee", () => {
    assert.equal(formatPaise(5), "₹0.05");
    assert.equal(formatPaise(50), "₹0.50");
    assert.equal(formatPaise(99), "₹0.99");
  });

  test("formats single and double digit rupees", () => {
    assert.equal(formatPaise(100), "₹1.00");
    assert.equal(formatPaise(9900), "₹99.00");
  });

  test("formats hundreds without commas", () => {
    assert.equal(formatPaise(99900), "₹999.00");
  });

  test("formats thousands with first comma at 3 digits (1,000)", () => {
    assert.equal(formatPaise(100000), "₹1,000.00");
    assert.equal(formatPaise(123456), "₹1,234.56");
  });

  test("formats lakhs with 2-digit grouping after first 3 digits (1,23,456.78)", () => {
    assert.equal(formatPaise(10000000), "₹1,00,000.00");
    assert.equal(formatPaise(12345678), "₹1,23,456.78");
  });

  test("formats crores correctly (1,00,00,000.00)", () => {
    assert.equal(formatPaise(1000000000), "₹1,00,00,000.00");
    assert.equal(formatPaise(1234567890), "₹1,23,45,678.90");
  });

  test("handles negative amounts correctly", () => {
    assert.equal(formatPaise(-500), "-₹5.00");
    assert.equal(formatPaise(-12345678), "-₹1,23,456.78");
  });
});

describe("amountToWords - Indian English Words Converter", () => {
  test("converts zero paise", () => {
    assert.equal(amountToWords(0), "Rupees Zero Only");
  });

  test("converts exact whole rupee amounts", () => {
    assert.equal(amountToWords(100), "Rupees One Only");
    assert.equal(amountToWords(500), "Rupees Five Only");
    assert.equal(amountToWords(1500), "Rupees Fifteen Only");
    assert.equal(amountToWords(10000), "Rupees One Hundred Only");
    assert.equal(amountToWords(100000), "Rupees One Thousand Only");
    assert.equal(amountToWords(10000000), "Rupees One Lakh Only");
    assert.equal(amountToWords(1000000000), "Rupees One Crore Only");
  });

  test("converts paise only amounts under 1 rupee", () => {
    assert.equal(amountToWords(50), "Fifty Paise Only");
    assert.equal(amountToWords(5), "Five Paise Only");
    assert.equal(amountToWords(99), "Ninety-Nine Paise Only".replace("-", " "));
  });

  test("converts combined rupees and paise", () => {
    assert.equal(amountToWords(150), "Rupees One and Fifty Paise Only");
    assert.equal(amountToWords(123450), "Rupees One Thousand Two Hundred Thirty Four and Fifty Paise Only");
  });

  test("converts multi-denomination complex amounts (Crores, Lakhs, Thousands)", () => {
    assert.equal(
      amountToWords(125000000),
      "Rupees Twelve Lakh Fifty Thousand Only"
    );
    assert.equal(
      amountToWords(1020304000),
      "Rupees One Crore Two Lakh Three Thousand Forty Only"
    );
  });

  test("handles negative amounts", () => {
    assert.equal(amountToWords(-100), "Minus Rupees One Only");
    assert.equal(amountToWords(-150), "Minus Rupees One and Fifty Paise Only");
  });
});

describe("calculateItemSubtotalPaise - Quantity Milli to Paise", () => {
  test("calculates exact subtotal for whole unit quantities", () => {
    // 1 unit (1000 milli) at ₹250.00 (25000 paise) = 25000 paise
    assert.equal(calculateItemSubtotalPaise(1000, 25000), 25000);
    // 5 units (5000 milli) at ₹49.99 (4999 paise) = 24995 paise
    assert.equal(calculateItemSubtotalPaise(5000, 4999), 24995);
  });

  test("calculates subtotal for fractional milli-unit quantities", () => {
    // 250 milli (0.250 kg) at ₹120.00 (12000 paise/kg) = 3000 paise (₹30.00)
    assert.equal(calculateItemSubtotalPaise(250, 12000), 3000);
    // 500 milli (0.500 l) at ₹75.50 (7550 paise/l) = 3775 paise (₹37.75)
    assert.equal(calculateItemSubtotalPaise(500, 7550), 3775);
  });

  test("rounds half-up deterministically on odd fractional division", () => {
    // 333 milli * 100 paise = 33300; (33300 + 500) / 1000 = 33800 / 1000 = 33 paise
    assert.equal(calculateItemSubtotalPaise(333, 100), 33);
    // 500 milli * 1 paise = 500; (500 + 500) / 1000 = 1000 / 1000 = 1 paise
    assert.equal(calculateItemSubtotalPaise(500, 1), 1);
    // 499 milli * 1 paise = 499; (499 + 500) / 1000 = 999 / 1000 = 0 paise
    assert.equal(calculateItemSubtotalPaise(499, 1), 0);
  });
});

describe("calculateLineTaxPaise - Basis Points to Paise Tax", () => {
  test("calculates zero tax for 0 bps rate", () => {
    assert.equal(calculateLineTaxPaise(10000, 0), 0);
    assert.equal(calculateLineTaxPaise(99999, 0), 0);
  });

  test("calculates exact tax for standard GST slabs", () => {
    // 5% (500 bps) on ₹100.00 (10000 paise) = 500 paise
    assert.equal(calculateLineTaxPaise(10000, 500), 500);
    // 12% (1200 bps) on ₹100.00 (10000 paise) = 1200 paise
    assert.equal(calculateLineTaxPaise(10000, 1200), 1200);
    // 18% (1800 bps) on ₹100.00 (10000 paise) = 1800 paise
    assert.equal(calculateLineTaxPaise(10000, 1800), 1800);
    // 28% (2800 bps) on ₹100.00 (10000 paise) = 2800 paise
    assert.equal(calculateLineTaxPaise(10000, 2800), 2800);
  });

  test("handles odd-paise rounding with nearest-paise tie-breaking (+5000 / 10000)", () => {
    // 1800 bps (18%) on 33 paise: (33 * 1800 + 5000) / 10000 = (59400 + 5000) / 10000 = 6 paise
    assert.equal(calculateLineTaxPaise(33, 1800), 6);
    // 1800 bps (18%) on 34 paise: (34 * 1800 + 5000) / 10000 = (61200 + 5000) / 10000 = 6 paise
    assert.equal(calculateLineTaxPaise(34, 1800), 6);
    // 1800 bps (18%) on 35 paise: (35 * 1800 + 5000) / 10000 = (63000 + 5000) / 10000 = 6 paise
    assert.equal(calculateLineTaxPaise(35, 1800), 6);
    // 1800 bps (18%) on 36 paise: (36 * 1800 + 5000) / 10000 = (64800 + 5000) / 10000 = 6 paise
    assert.equal(calculateLineTaxPaise(36, 1800), 6);
    // 1800 bps (18%) on 37 paise: (37 * 1800 + 5000) / 10000 = (66600 + 5000) / 10000 = 7 paise
    assert.equal(calculateLineTaxPaise(37, 1800), 7);
  });
});

describe("computeTaxSlabs - Authoritative Tax Slabs & Integer Derivation", () => {
  test("handles empty or nil items safely", () => {
    assert.deepEqual(computeTaxSlabs([]), {});
    // @ts-expect-error test null handling
    assert.deepEqual(computeTaxSlabs(null), {});
  });

  test("derives taxable paise strictly as line_total_paise - tax_paise without float math", () => {
    const items = [
      { gst_rate_bps: 1800, line_total_paise: 11800, tax_paise: 1800 },
    ];
    const slabs = computeTaxSlabs(items);
    assert.ok(slabs[1800]);
    assert.equal(slabs[1800].taxablePaise, 10000);
    assert.equal(slabs[1800].taxPaise, 1800);
    assert.equal(slabs[1800].rateBps, 1800);
    // Strict identity
    assert.equal(slabs[1800].taxablePaise + slabs[1800].taxPaise, items[0].line_total_paise);
  });

  test("aggregates multiple items across multi-rate GST slabs correctly", () => {
    const items = [
      { gst_rate_bps: 500, line_total_paise: 1050, tax_paise: 50 },
      { gst_rate_bps: 1800, line_total_paise: 1180, tax_paise: 180 },
      { gst_rate_bps: 1800, line_total_paise: 2360, tax_paise: 360 },
      { gst_rate_bps: 0, line_total_paise: 500, tax_paise: 0 },
    ];
    const slabs = computeTaxSlabs(items);

    // 0 bps slab
    assert.ok(slabs[0]);
    assert.equal(slabs[0].taxablePaise, 500);
    assert.equal(slabs[0].taxPaise, 0);

    // 500 bps slab
    assert.ok(slabs[500]);
    assert.equal(slabs[500].taxablePaise, 1000);
    assert.equal(slabs[500].taxPaise, 50);

    // 1800 bps slab (two items combined)
    assert.ok(slabs[1800]);
    assert.equal(slabs[1800].taxablePaise, 1000 + 2000); // 3000
    assert.equal(slabs[1800].taxPaise, 180 + 360); // 540

    // Exact conservation: sum of taxable + sum of tax equals sum of line totals
    const totalTaxable = Object.values(slabs).reduce((sum, s) => sum + s.taxablePaise, 0);
    const totalTax = Object.values(slabs).reduce((sum, s) => sum + s.taxPaise, 0);
    const totalLine = items.reduce((sum, item) => sum + item.line_total_paise, 0);
    assert.equal(totalTaxable + totalTax, totalLine);
  });

  test("guarantees zero remainder discrepancies on odd-paise amounts", () => {
    // Odd amount where standard division would produce infinite recurring fractions
    const items = [
      { gst_rate_bps: 1800, line_total_paise: 99, tax_paise: 15 },
      { gst_rate_bps: 1800, line_total_paise: 33, tax_paise: 5 },
      { gst_rate_bps: 1200, line_total_paise: 77, tax_paise: 8 },
    ];
    const slabs = computeTaxSlabs(items);

    assert.equal(slabs[1800].taxablePaise, (99 - 15) + (33 - 5)); // 84 + 28 = 112
    assert.equal(slabs[1800].taxPaise, 15 + 5); // 20
    assert.equal(slabs[1800].taxablePaise + slabs[1800].taxPaise, 99 + 33); // 132

    assert.equal(slabs[1200].taxablePaise, 77 - 8); // 69
    assert.equal(slabs[1200].taxPaise, 8);
    assert.equal(slabs[1200].taxablePaise + slabs[1200].taxPaise, 77);
  });
});

