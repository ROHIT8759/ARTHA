import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { formatPaise, amountToWords } from "../lib/api.ts";

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
