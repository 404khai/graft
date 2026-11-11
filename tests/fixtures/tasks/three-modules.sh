#!/bin/sh
# Writes the Python project that tests/fixtures/tasks/three-modules.md is a
# task for: three modules (billing, users, search) whose files are tightly
# coupled inside each module, plus app/types.py, which every module imports.
set -eu
dir=${1:?usage: three-modules.sh DIR}
mkdir -p "$dir/app" "$dir/billing" "$dir/users" "$dir/search"
cd "$dir"
: > app/__init__.py; : > billing/__init__.py; : > users/__init__.py; : > search/__init__.py
cat > app/types.py <<'PY'
class Money:
    def __init__(self, cents):
        self.cents = cents


class Query:
    def __init__(self, text):
        self.text = text
PY
cat > billing/invoice.py <<'PY'
from app.types import Money
from billing.tax import rate, round_cents


def total(lines):
    cents = sum(line.cents for line in lines)
    return Money(round_cents(cents + cents * rate()))


def subtotal(lines):
    return Money(round_cents(sum(line.cents for line in lines)))
PY
cat > billing/tax.py <<'PY'
from app.types import Money


def rate():
    return 0.2


def round_cents(cents):
    return int(cents + 0.5)


def tax_on(money):
    return Money(round_cents(money.cents * rate()))
PY
cat > users/models.py <<'PY'
from app.types import Money


class User:
    def __init__(self, name, credit):
        self.name = name
        self.credit = Money(credit)


def find(users, name):
    return [u for u in users if u.name == name]
PY
cat > users/auth.py <<'PY'
from app.types import Money
from users.models import User, find


def login(users, name):
    found = find(users, name)
    return found[0] if found else User(name, 0)


def credit_of(users, name):
    user = login(users, name)
    return user.credit if isinstance(user, User) else Money(0)
PY
cat > search/index.py <<'PY'
from app.types import Query


class Index:
    def __init__(self):
        self.docs = []

    def add(self, doc):
        self.docs.append(doc)
PY
cat > search/query.py <<'PY'
from app.types import Money, Query
from search.index import Index


def run(index, query):
    return [d for d in index.docs if query.text in d]


def build(docs):
    index = Index()
    for d in docs:
        index.add(d)
    return index, Query("")
PY
