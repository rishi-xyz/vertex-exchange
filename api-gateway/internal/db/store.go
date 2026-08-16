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
