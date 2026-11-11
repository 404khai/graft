# Add currencies to billing, sessions to users, and filters to search

Every amount needs a currency, so `Money` in `app/types.py` gains a
`currency` field. That is a shared type: land it first.

Then, in parallel:

- billing: `billing/invoice.py` totals per currency, and `billing/tax.py`
  rounds per currency.
- users: `users/models.py` gets a `Session`, and `users/auth.py` issues one
  on login.
- search: `search/index.py` stores the currency, and `search/query.py`
  filters by it.
