/**
 * Integration constants for bank sync providers
 */

// SimpleFIN
export const SIMPLEFIN = {
  name: "SimpleFIN",
  url: "https://beta-bridge.simplefin.org/",
  coverage: "US & Canada",
  pricing: "~$1.50/month or $15/year",
  description:
    "Sync bank accounts from US and Canadian institutions. ~$1.50/month for up to 25 connections.",
  shortDescription:
    "Connect your bank accounts via SimpleFIN to automatically sync transactions and balances.",
} as const;

// Lunch Flow
export const LUNCHFLOW = {
  name: "Lunch Flow",
  url: "https://www.lunchflow.app/?atp=treeline",
  coverage: "40+ countries (EU, UK, US, Canada, Asia, Brazil)",
  banks: "20,000+",
  pricing: "~$3/month with 7-day free trial",
  description:
    "Sync banks from 40+ countries including EU, UK, US, Canada, Asia, and Brazil. ~$3/month with 7-day free trial.",
  shortDescription:
    "Connect to 20,000+ banks across 40+ countries (EU, UK, US, Canada, Asia, Brazil) via Lunch Flow.",
} as const;
