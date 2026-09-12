package db

import (
	"context"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

type Store struct {
	pool *pgxpool.Pool
}

type User struct {
	ID           uuid.UUID
	Email        string
	PasswordHash string
	EngineUserID string
	CreatedAt    time.Time
}

func NewStore(pool *pgxpool.Pool) *Store {
	return &Store{pool: pool}
}

func (s *Store) Ping(ctx context.Context) error {
	return s.pool.Ping(ctx)
}

func (s *Store) CreateUser(ctx context.Context, email, passwordHash, engineUserID string) (uuid.UUID, error) {
	var id uuid.UUID
	err := s.pool.QueryRow(ctx,
		`INSERT INTO users (email, password_hash, engine_user_id) VALUES ($1, $2, $3) RETURNING id`,
		email, passwordHash, engineUserID,
	).Scan(&id)
	return id, err
}

func (s *Store) GetUserByEmail(ctx context.Context, email string) (*User, error) {
	u, err := s.getUser(ctx, `SELECT id, email, password_hash, engine_user_id, created_at FROM users WHERE email = $1`, email)
	if err == pgx.ErrNoRows {
		return nil, ErrNotFound
	}
	return u, err
}

func (s *Store) GetUserByID(ctx context.Context, id uuid.UUID) (*User, error) {
	u, err := s.getUser(ctx, `SELECT id, email, password_hash, engine_user_id, created_at FROM users WHERE id = $1`, id)
	if err == pgx.ErrNoRows {
		return nil, ErrNotFound
	}
	return u, err
}

func (s *Store) getUser(ctx context.Context, query string, arg any) (*User, error) {
	var u User
	err := s.pool.QueryRow(ctx, query, arg).Scan(
		&u.ID, &u.Email, &u.PasswordHash, &u.EngineUserID, &u.CreatedAt,
	)
	if err != nil {
		return nil, err
	}
	return &u, nil
}

type Order struct {
	ID            uuid.UUID
	UserID        uuid.UUID
	Pair          string
	Side          string
	Type          string
	Price         int64
	Quantity      int64
	Remaining     int64
	Status        string
	EngineOrderID int64
	CreatedAt     time.Time
}

func (s *Store) CreateOrder(ctx context.Context, userID uuid.UUID, pair, side, orderType string,
	price int32, quantity uint32, engineOrderID int64) (uuid.UUID, error) {
	var id uuid.UUID
	err := s.pool.QueryRow(ctx,
		`INSERT INTO orders (user_id, pair, side, order_type, price, quantity, remaining, status, engine_order_id)
		 VALUES ($1, $2, $3, $4, $5, $6, $6, 'Empty', $7) RETURNING id`,
		userID, pair, side, orderType, price, quantity, engineOrderID,
	).Scan(&id)
	return id, err
}

func (s *Store) GetOrderByID(ctx context.Context, id uuid.UUID) (*Order, error) {
	var o Order
	err := s.pool.QueryRow(ctx,
		`SELECT id, user_id, pair, side, order_type, price, quantity, remaining, status, engine_order_id, created_at
		 FROM orders WHERE id = $1`, id,
	).Scan(&o.ID, &o.UserID, &o.Pair, &o.Side, &o.Type, &o.Price, &o.Quantity, &o.Remaining, &o.Status, &o.EngineOrderID, &o.CreatedAt)
	if err == pgx.ErrNoRows {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	return &o, nil
}

// ListOrders returns a user's orders, most recent first, optionally filtered
// by pair and/or status. Empty filters match everything.
func (s *Store) ListOrders(ctx context.Context, userID uuid.UUID, pair, status string, limit, offset int) ([]*Order, error) {
	rows, err := s.pool.Query(ctx,
		`SELECT id, user_id, pair, side, order_type, price, quantity, remaining, status, engine_order_id, created_at
		 FROM orders
		 WHERE user_id = $1 AND ($2 = '' OR pair = $2) AND ($3 = '' OR status = $3)
		 ORDER BY created_at DESC
		 LIMIT $4 OFFSET $5`,
		userID, pair, status, limit, offset,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var orders []*Order
	for rows.Next() {
		var o Order
		if err := rows.Scan(&o.ID, &o.UserID, &o.Pair, &o.Side, &o.Type, &o.Price, &o.Quantity, &o.Remaining, &o.Status, &o.EngineOrderID, &o.CreatedAt); err != nil {
			return nil, err
		}
		orders = append(orders, &o)
	}
	return orders, rows.Err()
}

// ListOpenOrders returns every order not yet in a terminal state, across all
// users. Used once at gateway boot to hydrate the in-memory live-order
// registry (internal/liveorders) so orders that were already open before a
// restart still get hot-path fill notifications.
func (s *Store) ListOpenOrders(ctx context.Context) ([]*Order, error) {
	rows, err := s.pool.Query(ctx,
		`SELECT id, user_id, pair, side, order_type, price, quantity, remaining, status, engine_order_id, created_at
		 FROM orders WHERE status IN ('Empty', 'PartiallyFilled')`,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var orders []*Order
	for rows.Next() {
		var o Order
		if err := rows.Scan(&o.ID, &o.UserID, &o.Pair, &o.Side, &o.Type, &o.Price, &o.Quantity, &o.Remaining, &o.Status, &o.EngineOrderID, &o.CreatedAt); err != nil {
			return nil, err
		}
		orders = append(orders, &o)
	}
	return orders, rows.Err()
}

// CancelOrder marks an order Cancelled. It is a no-op if the order is already
// in a terminal state (Filled/Cancelled).
func (s *Store) CancelOrder(ctx context.Context, id uuid.UUID) error {
	_, err := s.pool.Exec(ctx,
		`UPDATE orders SET status = 'Cancelled' WHERE id = $1 AND status NOT IN ('Filled', 'Cancelled')`, id)
	return err
}

// ModifyOrder overwrites an order's price/quantity after a successful
// engine cancel-replace, resetting it to a fresh resting order.
func (s *Store) ModifyOrder(ctx context.Context, id uuid.UUID, price int32, quantity uint32) error {
	_, err := s.pool.Exec(ctx,
		`UPDATE orders SET price = $2, quantity = $3, remaining = $3, status = 'Empty' WHERE id = $1`,
		id, price, quantity)
	return err
}
