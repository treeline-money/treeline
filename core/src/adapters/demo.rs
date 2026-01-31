//! Demo data provider for testing
//!
//! Generates realistic demo data matching Python CLI behavior:
//! - 6 accounts with proper balances
//! - 180 days of transactions with realistic patterns
//! - 180 days of balance history for all accounts

use std::f64::consts::PI;

use chrono::{Datelike, Duration, NaiveDate, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::domain::{Account, BalanceSnapshot, Transaction};

/// Generate demo accounts
pub fn generate_demo_accounts() -> Vec<Account> {
    let now = Utc::now();

    vec![
        Account {
            id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            name: "Primary Checking".to_string(),
            nickname: Some("Everyday Spending".to_string()),
            account_type: Some("depository".to_string()),
            classification: Some("asset".to_string()),
            currency: "USD".to_string(),
            balance: Some(Decimal::new(482347, 2)), // $4,823.47
            institution_name: Some("Chase".to_string()),
            institution_url: Some("https://chase.com".to_string()),
            institution_domain: Some("chase.com".to_string()),
            created_at: now,
            updated_at: now,
            // Demo accounts are identified by name for deduplication
            is_manual: false,
            sf_id: None,
            sf_name: None,
            sf_currency: None,
            sf_balance: None,
            sf_available_balance: None,
            sf_balance_date: None,
            sf_org_name: None,
            sf_org_url: None,
            sf_org_domain: None,
            sf_extra: None,
            lf_id: None,
            lf_name: None,
            lf_institution_name: None,
            lf_institution_logo: None,
            lf_provider: None,
            lf_currency: None,
            lf_status: None,
        },
        Account {
            id: Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
            name: "High-Yield Savings".to_string(),
            nickname: Some("Emergency Fund".to_string()),
            account_type: Some("depository".to_string()),
            classification: Some("asset".to_string()),
            currency: "USD".to_string(),
            balance: Some(Decimal::new(1875000, 2)), // $18,750.00
            institution_name: Some("Marcus by Goldman Sachs".to_string()),
            institution_url: Some("https://marcus.com".to_string()),
            institution_domain: Some("marcus.com".to_string()),
            created_at: now,
            updated_at: now,
            is_manual: false,
            sf_id: None,
            sf_name: None,
            sf_currency: None,
            sf_balance: None,
            sf_available_balance: None,
            sf_balance_date: None,
            sf_org_name: None,
            sf_org_url: None,
            sf_org_domain: None,
            sf_extra: None,
            lf_id: None,
            lf_name: None,
            lf_institution_name: None,
            lf_institution_logo: None,
            lf_provider: None,
            lf_currency: None,
            lf_status: None,
        },
        Account {
            id: Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap(),
            name: "Sapphire Reserve".to_string(),
            nickname: Some("Travel Card".to_string()),
            account_type: Some("credit".to_string()),
            classification: Some("liability".to_string()),
            currency: "USD".to_string(),
            balance: Some(Decimal::new(-284763, 2)), // -$2,847.63
            institution_name: Some("Chase".to_string()),
            institution_url: Some("https://chase.com".to_string()),
            institution_domain: Some("chase.com".to_string()),
            created_at: now,
            updated_at: now,
            is_manual: false,
            sf_id: None,
            sf_name: None,
            sf_currency: None,
            sf_balance: None,
            sf_available_balance: None,
            sf_balance_date: None,
            sf_org_name: None,
            sf_org_url: None,
            sf_org_domain: None,
            sf_extra: None,
            lf_id: None,
            lf_name: None,
            lf_institution_name: None,
            lf_institution_logo: None,
            lf_provider: None,
            lf_currency: None,
            lf_status: None,
        },
        Account {
            id: Uuid::parse_str("44444444-4444-4444-4444-444444444444").unwrap(),
            name: "Citi Double Cash".to_string(),
            nickname: Some("Cashback Card".to_string()),
            account_type: Some("credit".to_string()),
            classification: Some("liability".to_string()),
            currency: "USD".to_string(),
            balance: Some(Decimal::new(-124589, 2)), // -$1,245.89
            institution_name: Some("Citi".to_string()),
            institution_url: Some("https://citi.com".to_string()),
            institution_domain: Some("citi.com".to_string()),
            created_at: now,
            updated_at: now,
            is_manual: false,
            sf_id: None,
            sf_name: None,
            sf_currency: None,
            sf_balance: None,
            sf_available_balance: None,
            sf_balance_date: None,
            sf_org_name: None,
            sf_org_url: None,
            sf_org_domain: None,
            sf_extra: None,
            lf_id: None,
            lf_name: None,
            lf_institution_name: None,
            lf_institution_logo: None,
            lf_provider: None,
            lf_currency: None,
            lf_status: None,
        },
        Account {
            id: Uuid::parse_str("55555555-5555-5555-5555-555555555555").unwrap(),
            name: "Individual Brokerage".to_string(),
            nickname: Some("Investments".to_string()),
            account_type: Some("investment".to_string()),
            classification: Some("asset".to_string()),
            currency: "USD".to_string(),
            balance: Some(Decimal::new(4782315, 2)), // $47,823.15
            institution_name: Some("Fidelity".to_string()),
            institution_url: Some("https://fidelity.com".to_string()),
            institution_domain: Some("fidelity.com".to_string()),
            created_at: now,
            updated_at: now,
            is_manual: false,
            sf_id: None,
            sf_name: None,
            sf_currency: None,
            sf_balance: None,
            sf_available_balance: None,
            sf_balance_date: None,
            sf_org_name: None,
            sf_org_url: None,
            sf_org_domain: None,
            sf_extra: None,
            lf_id: None,
            lf_name: None,
            lf_institution_name: None,
            lf_institution_logo: None,
            lf_provider: None,
            lf_currency: None,
            lf_status: None,
        },
        Account {
            id: Uuid::parse_str("66666666-6666-6666-6666-666666666666").unwrap(),
            name: "401(k)".to_string(),
            nickname: Some("Retirement".to_string()),
            account_type: Some("investment".to_string()),
            classification: Some("asset".to_string()),
            currency: "USD".to_string(),
            balance: Some(Decimal::new(8943267, 2)), // $89,432.67
            institution_name: Some("Fidelity".to_string()),
            institution_url: Some("https://fidelity.com".to_string()),
            institution_domain: Some("fidelity.com".to_string()),
            created_at: now,
            updated_at: now,
            is_manual: false,
            sf_id: None,
            sf_name: None,
            sf_currency: None,
            sf_balance: None,
            sf_available_balance: None,
            sf_balance_date: None,
            sf_org_name: None,
            sf_org_url: None,
            sf_org_domain: None,
            sf_extra: None,
            lf_id: None,
            lf_name: None,
            lf_institution_name: None,
            lf_institution_logo: None,
            lf_provider: None,
            lf_currency: None,
            lf_status: None,
        },
    ]
}

/// Generate demo transactions (180 days of realistic data)
pub fn generate_demo_transactions() -> Vec<Transaction> {
    let now = Utc::now();
    let today = now.date_naive();
    let checking_id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    let credit_chase_id = Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap();
    let credit_citi_id = Uuid::parse_str("44444444-4444-4444-4444-444444444444").unwrap();

    let mut transactions = Vec::new();

    // Generate 180 days of transactions
    for days_ago in 0..180 {
        let date = today - Duration::days(days_ago);
        let day_of_month = date.day();

        // Paycheck on 1st and 15th (to checking)
        if day_of_month == 1 || day_of_month == 15 {
            transactions.push(create_transaction(
                checking_id,
                date,
                Decimal::new(425000, 2), // $4,250.00
                "ACME CORP PAYROLL DIRECT DEPOSIT",
                vec!["income".to_string(), "salary".to_string()],
                now,
            ));
        }

        // Rent on 5th (from checking)
        if day_of_month == 5 {
            transactions.push(create_transaction(
                checking_id,
                date,
                Decimal::new(-225000, 2), // -$2,250.00
                "APARTMENT RENT PAYMENT",
                vec!["rent".to_string(), "housing".to_string()],
                now,
            ));
        }

        // Utilities on 10th (from checking)
        if day_of_month == 10 {
            transactions.push(create_transaction(
                checking_id,
                date,
                Decimal::new(-15000, 2), // -$150.00
                "CITY UTILITIES - ELECTRIC",
                vec!["utilities".to_string()],
                now,
            ));
            transactions.push(create_transaction(
                checking_id,
                date,
                Decimal::new(-7500, 2), // -$75.00
                "COMCAST INTERNET",
                vec!["utilities".to_string(), "internet".to_string()],
                now,
            ));
        }

        // Savings transfer on 16th (from checking)
        if day_of_month == 16 {
            transactions.push(create_transaction(
                checking_id,
                date,
                Decimal::new(-75000, 2), // -$750.00
                "TRANSFER TO SAVINGS",
                vec!["transfer".to_string(), "savings".to_string()],
                now,
            ));
        }

        // Insurance on 20th (from checking)
        if day_of_month == 20 {
            transactions.push(create_transaction(
                checking_id,
                date,
                Decimal::new(-18500, 2), // -$185.00
                "STATE FARM AUTO INSURANCE",
                vec!["insurance".to_string(), "auto".to_string()],
                now,
            ));
        }

        // Credit card payments on 25th (Chase) and 20th (Citi)
        if day_of_month == 25 {
            transactions.push(create_transaction(
                checking_id,
                date,
                Decimal::new(-250000, 2), // -$2,500.00
                "CHASE CREDIT CARD PAYMENT",
                vec!["payment".to_string()],
                now,
            ));
        }
        if day_of_month == 20 {
            transactions.push(create_transaction(
                checking_id,
                date,
                Decimal::new(-100000, 2), // -$1,000.00
                "CITI CREDIT CARD PAYMENT",
                vec!["payment".to_string()],
                now,
            ));
        }

        // Groceries every 3-4 days (on Chase card)
        if days_ago % 3 == 0 {
            let amounts = [-8523i64, -6745, -9234, -7100, -5899, -10523];
            let amount = amounts[days_ago as usize % amounts.len()];
            transactions.push(create_transaction(
                credit_chase_id,
                date,
                Decimal::new(amount, 2),
                "WHOLE FOODS MARKET",
                vec!["groceries".to_string(), "food".to_string()],
                now,
            ));
        }

        // Coffee every 2 days (on Citi card)
        if days_ago % 2 == 0 {
            transactions.push(create_transaction(
                credit_citi_id,
                date,
                Decimal::new(-565, 2), // -$5.65
                "STARBUCKS",
                vec!["coffee".to_string(), "food".to_string()],
                now,
            ));
        }

        // Dining out twice a week (on Chase card)
        if days_ago % 3 == 1 || days_ago % 7 == 0 {
            let restaurants = [
                ("CHIPOTLE MEXICAN GRILL", -1250i64),
                ("SWEETGREEN", -1450),
                ("THE CAPITAL GRILLE", -8500),
                ("PHO RESTAURANTS", -2200),
                ("SHAKE SHACK", -1875),
            ];
            let (name, amount) = restaurants[days_ago as usize % restaurants.len()];
            transactions.push(create_transaction(
                credit_chase_id,
                date,
                Decimal::new(amount, 2),
                name,
                vec!["dining".to_string(), "food".to_string()],
                now,
            ));
        }

        // Gas every 7 days (on Citi card)
        if days_ago % 7 == 0 {
            transactions.push(create_transaction(
                credit_citi_id,
                date,
                Decimal::new(-5500, 2), // -$55.00
                "SHELL OIL",
                vec!["gas".to_string(), "transportation".to_string()],
                now,
            ));
        }

        // Subscriptions on various days
        if day_of_month == 3 {
            transactions.push(create_transaction(
                credit_chase_id,
                date,
                Decimal::new(-1599, 2), // -$15.99
                "NETFLIX",
                vec!["entertainment".to_string(), "subscription".to_string()],
                now,
            ));
        }
        if day_of_month == 7 {
            transactions.push(create_transaction(
                credit_chase_id,
                date,
                Decimal::new(-1099, 2), // -$10.99
                "SPOTIFY PREMIUM",
                vec!["entertainment".to_string(), "subscription".to_string()],
                now,
            ));
        }
        if day_of_month == 12 {
            transactions.push(create_transaction(
                credit_citi_id,
                date,
                Decimal::new(-999, 2), // -$9.99
                "AMAZON PRIME",
                vec!["subscription".to_string()],
                now,
            ));
        }
        if day_of_month == 15 {
            transactions.push(create_transaction(
                credit_citi_id,
                date,
                Decimal::new(-4999, 2), // -$49.99
                "GYM MEMBERSHIP",
                vec!["health".to_string(), "fitness".to_string()],
                now,
            ));
        }

        // Shopping every 5 days (on Chase card)
        if days_ago % 5 == 0 {
            let shops = [
                ("AMAZON.COM", -3299i64),
                ("TARGET", -7850),
                ("BEST BUY", -12999),
                ("NORDSTROM", -18500),
                ("HOME DEPOT", -8725),
            ];
            let (name, amount) = shops[days_ago as usize % shops.len()];
            transactions.push(create_transaction(
                credit_chase_id,
                date,
                Decimal::new(amount, 2),
                name,
                vec!["shopping".to_string()],
                now,
            ));
        }
    }

    transactions
}

fn create_transaction(
    account_id: Uuid,
    date: NaiveDate,
    amount: Decimal,
    description: &str,
    tags: Vec<String>,
    now: chrono::DateTime<Utc>,
) -> Transaction {
    let mut tx = Transaction::new(Uuid::new_v4(), account_id, amount, date);
    tx.description = Some(description.to_string());
    tx.tags = tags;
    tx.created_at = now;
    tx.updated_at = now;
    // Fingerprint will be calculated by the sync service for deduplication
    tx
}

/// Simple deterministic random number generator (LCG)
/// Uses fixed seed for reproducibility
struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> f64 {
        // Linear congruential generator
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        // Convert to 0.0-1.0 range
        (self.state >> 32) as f64 / u32::MAX as f64
    }
}

/// Generate demo balance snapshots (180 days of history for all accounts)
pub fn generate_demo_balance_snapshots() -> Vec<BalanceSnapshot> {
    let now = Utc::now();
    let today = now.date_naive();

    // Account configurations: (uuid, current_balance, balance_type, volatility)
    let account_configs = [
        (
            "11111111-1111-1111-1111-111111111111",
            4823.47_f64,
            "checking",
            0.02,
        ),
        (
            "22222222-2222-2222-2222-222222222222",
            18750.00,
            "savings",
            0.01,
        ),
        (
            "33333333-3333-3333-3333-333333333333",
            -2847.63,
            "credit_chase",
            0.15,
        ),
        (
            "44444444-4444-4444-4444-444444444444",
            -1245.89,
            "credit_citi",
            0.12,
        ),
        (
            "55555555-5555-5555-5555-555555555555",
            47823.15,
            "market",
            0.08,
        ),
        (
            "66666666-6666-6666-6666-666666666666",
            89432.67,
            "market",
            0.06,
        ),
    ];

    let mut snapshots = Vec::new();
    let mut rng = SimpleRng::new(42); // Fixed seed for reproducibility

    for (uuid_str, current_balance, balance_type, volatility) in &account_configs {
        let account_id = Uuid::parse_str(uuid_str).unwrap();
        let mut balance = *current_balance;

        // Generate 180 days of history going backward
        for day in 0..180 {
            let date = today - Duration::days(day);
            let day_of_month = date.day();

            match *balance_type {
                "market" => {
                    // Investment: Market fluctuations with upward trend
                    // Working backward: so we divide by growth factor to get past values
                    let daily_growth = 1.0 + (0.10 / 365.0); // ~10% annual
                    let cycle = (day as f64 * 2.0 * PI / 60.0).sin() * 0.02; // 60-day cycles
                    let noise = (rng.next() - 0.5) * volatility * 0.3;
                    let daily_factor = daily_growth + cycle + noise;
                    balance /= daily_factor;

                    // Occasional larger moves (3% chance)
                    if rng.next() < 0.03 {
                        balance *= 1.0 + (rng.next() - 0.5) * 0.05;
                    }
                }
                "savings" => {
                    // Savings: Gradual growth with monthly deposits
                    if day_of_month == 16 {
                        balance -= 750.0; // Remove transfer going backward
                    }
                    if day_of_month == 28 {
                        // Remove monthly interest
                        let monthly_interest = balance * (0.04 / 12.0);
                        balance -= monthly_interest;
                    }
                    balance *= 1.0 + (rng.next() - 0.5) * 0.002;
                }
                "credit_chase" => {
                    // Chase credit card: paid on 25th
                    if day_of_month == 25 {
                        balance = current_balance * 0.15; // After payment
                    } else if day_of_month < 25 {
                        let progress = day_of_month as f64 / 25.0;
                        balance = current_balance * (0.15 + 0.85 * progress);
                    } else {
                        let progress = (day_of_month - 25) as f64 / 5.0;
                        balance = current_balance * (0.15 + 0.3 * progress);
                    }
                    balance *= 1.0 + (rng.next() - 0.5) * volatility;
                }
                "credit_citi" => {
                    // Citi credit card: paid on 20th
                    if day_of_month == 20 {
                        balance = current_balance * 0.10; // After payment
                    } else if day_of_month < 20 {
                        let progress = day_of_month as f64 / 20.0;
                        balance = current_balance * (0.10 + 0.90 * progress);
                    } else {
                        let progress = (day_of_month - 20) as f64 / 10.0;
                        balance = current_balance * (0.10 + 0.35 * progress);
                    }
                    balance *= 1.0 + (rng.next() - 0.5) * volatility;
                }
                "checking" => {
                    // Checking: Paycheck cycles + bill payments
                    if day_of_month == 1 || day_of_month == 15 {
                        balance = current_balance * 1.8; // After paycheck
                    } else if day_of_month == 2 || day_of_month == 16 {
                        balance = current_balance * 1.6; // Day after paycheck
                    } else if day_of_month == 5 {
                        balance = current_balance * 0.7; // After rent
                    } else {
                        let base_ratio = 0.8 + (rng.next() * 0.6);
                        balance = current_balance * base_ratio;
                    }
                }
                _ => {}
            }

            // Create snapshot at end of day
            let snapshot_time = date.and_hms_opt(23, 59, 59).unwrap();
            snapshots.push(BalanceSnapshot {
                id: Uuid::new_v4(),
                account_id,
                balance: Decimal::new((balance * 100.0).round() as i64, 2),
                snapshot_time,
                source: Some("sync".to_string()),
                created_at: now,
                updated_at: now,
            });
        }
    }

    snapshots
}

// =============================================================================
// DemoDataProvider - implements DataAggregationProvider trait
// =============================================================================

use crate::domain::result::Result;
use crate::ports::{
    DataAggregationProvider, FetchAccountsResult, FetchTransactionsResult, IntegrationProvider,
};
use serde_json::Value as JsonValue;

/// Demo data provider
///
/// Implements DataAggregationProvider and IntegrationProvider traits
/// for generating realistic demo data.
pub struct DemoDataProvider;

impl DemoDataProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DemoDataProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl DataAggregationProvider for DemoDataProvider {
    fn name(&self) -> &str {
        "demo"
    }

    fn can_get_accounts(&self) -> bool {
        true
    }

    fn can_get_transactions(&self) -> bool {
        true
    }

    fn can_get_balances(&self) -> bool {
        true
    }

    fn get_accounts(&self, _settings: &JsonValue) -> Result<FetchAccountsResult> {
        Ok(FetchAccountsResult {
            accounts: generate_demo_accounts(),
            balance_snapshots: generate_demo_balance_snapshots(),
            warnings: Vec::new(),
        })
    }

    fn get_transactions(
        &self,
        _start_date: NaiveDate,
        _end_date: NaiveDate,
        _account_ids: &[String],
        _settings: &JsonValue,
    ) -> Result<FetchTransactionsResult> {
        let transactions = generate_demo_transactions();

        // Convert to (provider_account_id, Transaction) pairs
        // For demo mode, we use account name as the provider ID (matched in sync service)
        let accounts = generate_demo_accounts();
        let account_id_to_name: std::collections::HashMap<Uuid, String> =
            accounts.into_iter().map(|a| (a.id, a.name)).collect();

        let txs_with_ids: Vec<(String, Transaction)> = transactions
            .into_iter()
            .map(|tx| {
                // Use account name as provider account ID for demo mode
                let provider_account_id = account_id_to_name
                    .get(&tx.account_id)
                    .cloned()
                    .unwrap_or_else(|| tx.account_id.to_string());
                (provider_account_id, tx)
            })
            .collect();

        Ok(FetchTransactionsResult {
            transactions: txs_with_ids,
            warnings: Vec::new(),
        })
    }
}

impl IntegrationProvider for DemoDataProvider {
    fn setup(&self, _options: &JsonValue) -> Result<JsonValue> {
        // Demo integration needs no configuration
        Ok(serde_json::json!({}))
    }
}
