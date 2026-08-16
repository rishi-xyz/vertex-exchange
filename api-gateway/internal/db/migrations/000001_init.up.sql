CREATE TABLE users (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    email           text NOT NULL UNIQUE,
    password_hash   text NOT NULL,
    engine_user_id  text NOT NULL UNIQUE,
    created_at      timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE orders (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         uuid NOT NULL REFERENCES users(id),
    pair            text NOT NULL,
    side            text NOT NULL CHECK (side IN ('Buy', 'Sell')),
    order_type      text NOT NULL CHECK (order_type IN ('GoodTillCancel', 'GoodForDay', 'FillAndKill', 'FillOrKill')),
    price           bigint NOT NULL,
    quantity        bigint NOT NULL,
    remaining       bigint NOT NULL,
    status          text NOT NULL CHECK (status IN ('Empty', 'PartiallyFilled', 'Filled', 'Cancelled')),
    engine_order_id bigint NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_orders_user_id ON orders (user_id);
CREATE INDEX idx_orders_pair ON orders (pair);

CREATE TABLE trades (
    id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    trade_id      bigint NOT NULL UNIQUE,
    pair          text NOT NULL,
    price         bigint NOT NULL,
    quantity      bigint NOT NULL,
    bid_order_id  bigint NOT NULL,
    ask_order_id  bigint NOT NULL,
    bid_user_id   text NOT NULL,
    ask_user_id   text NOT NULL,
    executed_at   timestamptz NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_trades_pair ON trades (pair);
CREATE INDEX idx_trades_bid_user ON trades (bid_user_id);
CREATE INDEX idx_trades_ask_user ON trades (ask_user_id);
