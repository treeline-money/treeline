/**
 * Currency formatting utilities for Treeline UI.
 *
 * MVP: Single currency per user - user picks their currency upfront and
 * all their accounts must be in that currency.
 */

// Supported currencies with their symbols and locales
export const SUPPORTED_CURRENCIES: Record<
  string,
  { symbol: string; locale: string; name: string }
> = {
  USD: { symbol: "$", locale: "en-US", name: "US Dollar" },
  EUR: { symbol: "€", locale: "de-DE", name: "Euro" },
  GBP: { symbol: "£", locale: "en-GB", name: "British Pound" },
  CAD: { symbol: "$", locale: "en-CA", name: "Canadian Dollar" },
  AUD: { symbol: "$", locale: "en-AU", name: "Australian Dollar" },
  JPY: { symbol: "¥", locale: "ja-JP", name: "Japanese Yen" },
  CHF: { symbol: "CHF", locale: "de-CH", name: "Swiss Franc" },
  CNY: { symbol: "¥", locale: "zh-CN", name: "Chinese Yuan" },
  INR: { symbol: "₹", locale: "en-IN", name: "Indian Rupee" },
  MXN: { symbol: "$", locale: "es-MX", name: "Mexican Peso" },
  BRL: { symbol: "R$", locale: "pt-BR", name: "Brazilian Real" },
  KRW: { symbol: "₩", locale: "ko-KR", name: "South Korean Won" },
  SGD: { symbol: "$", locale: "en-SG", name: "Singapore Dollar" },
  HKD: { symbol: "$", locale: "zh-HK", name: "Hong Kong Dollar" },
  NOK: { symbol: "kr", locale: "nb-NO", name: "Norwegian Krone" },
  SEK: { symbol: "kr", locale: "sv-SE", name: "Swedish Krona" },
  DKK: { symbol: "kr", locale: "da-DK", name: "Danish Krone" },
  NZD: { symbol: "$", locale: "en-NZ", name: "New Zealand Dollar" },
  ZAR: { symbol: "R", locale: "en-ZA", name: "South African Rand" },
  PLN: { symbol: "zł", locale: "pl-PL", name: "Polish Zloty" },
};

export const DEFAULT_CURRENCY = "USD";

/**
 * Get the symbol for a currency code.
 */
export function getCurrencySymbol(currency: string): string {
  const upper = currency.toUpperCase();
  return SUPPORTED_CURRENCIES[upper]?.symbol ?? currency;
}

/**
 * Get the locale for a currency code.
 */
export function getCurrencyLocale(currency: string): string {
  const upper = currency.toUpperCase();
  return SUPPORTED_CURRENCIES[upper]?.locale ?? "en-US";
}

/**
 * Format an amount in the specified currency.
 */
export function formatCurrency(
  amount: number,
  currency: string = DEFAULT_CURRENCY,
  options: {
    minimumFractionDigits?: number;
    maximumFractionDigits?: number;
  } = {}
): string {
  const { minimumFractionDigits = 2, maximumFractionDigits = 2 } = options;
  const locale = getCurrencyLocale(currency);

  return new Intl.NumberFormat(locale, {
    style: "currency",
    currency: currency.toUpperCase(),
    minimumFractionDigits,
    maximumFractionDigits,
  }).format(amount);
}

/**
 * Format a large amount compactly (e.g., $1.2M, $500K).
 */
export function formatCurrencyCompact(
  amount: number,
  currency: string = DEFAULT_CURRENCY
): string {
  const symbol = getCurrencySymbol(currency);
  const absAmount = Math.abs(amount);
  const isNegative = amount < 0;

  let formatted: string;
  if (absAmount >= 1_000_000) {
    formatted = `${symbol}${(absAmount / 1_000_000).toFixed(1)}M`;
  } else if (absAmount >= 1_000) {
    formatted = `${symbol}${(absAmount / 1_000).toFixed(1)}K`;
  } else {
    formatted = `${symbol}${absAmount.toFixed(0)}`;
  }

  return isNegative ? `-${formatted}` : formatted;
}

/**
 * Format an amount without currency symbol (just the number).
 */
export function formatAmount(
  amount: number,
  options: {
    minimumFractionDigits?: number;
    maximumFractionDigits?: number;
  } = {}
): string {
  const { minimumFractionDigits = 2, maximumFractionDigits = 2 } = options;

  return new Intl.NumberFormat("en-US", {
    minimumFractionDigits,
    maximumFractionDigits,
  }).format(amount);
}

/**
 * Format cents (integer) to currency display.
 * Useful when storing amounts as integers.
 */
export function formatCurrencyCents(
  cents: number,
  currency: string = DEFAULT_CURRENCY
): string {
  return formatCurrency(cents / 100, currency);
}
