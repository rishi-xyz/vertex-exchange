package server

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"net/http"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/go-chi/chi/v5/middleware"
	"github.com/google/uuid"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	"github.com/rishi-xyz/vertex-exchange/api-gateway/gen/engine"
	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/auth"
	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/config"
	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/db"
	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/grpcclient"
	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/ws"
)

type Server struct {
	cfg    *config.Config
	engine *grpcclient.Client
	store  *db.Store
	auth   *auth.Manager
	hub    *ws.Hub
}

func New(cfg *config.Config, engine *grpcclient.Client, store *db.Store) *Server {
	return &Server{
		cfg:    cfg,
		engine: engine,
		store:  store,
		auth:   auth.NewManager(cfg.JWTSecret, cfg.JWTTTL),
		hub:    ws.NewHub(),
	}
}

func (s *Server) Router() http.Handler {
	r := chi.NewRouter()
	r.Use(middleware.RequestID)
	r.Use(middleware.RealIP)
	r.Use(middleware.Recoverer)

	r.Get("/healthz", s.handleHealthz)
	r.Route("/auth", func(r chi.Router) {
		r.Post("/register", s.handleRegister)
		r.Post("/login", s.handleLogin)
		r.With(s.authenticate).Get("/me", s.handleMe)
	})
	r.Route("/users", func(r chi.Router) {
		r.Post("/{id}/deposit", s.handleDeposit)
	})
	r.Route("/orders", func(r chi.Router) {
		r.Use(s.authenticate)
		r.Post("/", s.handleSubmitOrder)
		r.Get("/{id}", s.handleGetOrder)
	})
	r.With(s.authenticate).Get("/balances", s.handleBalances)
	r.Get("/orderbook/{pair}", s.handleGetOrderBook)
	r.Get("/ws", s.authenticateWS(s.handleWS))
	return r
}

func (s *Server) handleHealthz(w http.ResponseWriter, r *http.Request) {
	ctx, cancel := context.WithTimeout(r.Context(), 3*time.Second)
	defer cancel()

	if err := s.engine.Ping(ctx); err != nil {
		writeError(w, http.StatusServiceUnavailable, "engine_unreachable", "engine is not reachable")
		return
	}
	if err := s.store.Ping(ctx); err != nil {
		writeError(w, http.StatusServiceUnavailable, "database_unavailable", "database is not reachable")
		return
	}
	writeJSON(w, http.StatusOK, map[string]string{"status": "ok"})
}

func (s *Server) handleRegister(w http.ResponseWriter, r *http.Request) {
	var req struct {
		Email    string `json:"email"`
		Password string `json:"password"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid_request", "malformed JSON body")
		return
	}
	if !validEmail(req.Email) {
		writeError(w, http.StatusBadRequest, "invalid_email", "email is invalid")
		return
	}
	if len(req.Password) < 8 {
		writeError(w, http.StatusBadRequest, "invalid_password", "password must be at least 8 characters")
		return
	}

	if _, err := s.store.GetUserByEmail(r.Context(), req.Email); err == nil {
		writeError(w, http.StatusConflict, "email_taken", "email is already registered")
		return
	} else if !errors.Is(err, db.ErrNotFound) {
		s.internalError(w, "get user by email", err)
		return
	}

	engineUID := uuid.NewString()
	ctx, cancel := context.WithTimeout(r.Context(), 5*time.Second)
	defer cancel()
	if _, err := s.engine.Engine.AddUser(ctx, &engine.AddUserRequest{UserId: engineUID}); err != nil {
		writeError(w, http.StatusServiceUnavailable, "engine_unavailable", "engine is not reachable")
		return
	}

	hash, err := auth.HashPassword(req.Password)
	if err != nil {
		s.internalError(w, "hash password", err)
		return
	}
	userID, err := s.store.CreateUser(r.Context(), req.Email, hash, engineUID)
	if err != nil {
		s.internalError(w, "create user", err)
		return
	}

	writeJSON(w, http.StatusCreated, map[string]any{
		"user": s.userJSON(userID, req.Email, engineUID),
	})
}

func (s *Server) handleLogin(w http.ResponseWriter, r *http.Request) {
	var req struct {
		Email    string `json:"email"`
		Password string `json:"password"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid_request", "malformed JSON body")
		return
	}

	user, err := s.store.GetUserByEmail(r.Context(), req.Email)
	if errors.Is(err, db.ErrNotFound) {
		writeError(w, http.StatusUnauthorized, "invalid_credentials", "email or password is incorrect")
		return
	}
	if err != nil {
		s.internalError(w, "get user by email", err)
		return
	}
	if !auth.CheckPassword(user.PasswordHash, req.Password) {
		writeError(w, http.StatusUnauthorized, "invalid_credentials", "email or password is incorrect")
		return
	}

	token, exp, err := s.auth.Issue(user.ID.String(), user.EngineUserID)
	if err != nil {
		s.internalError(w, "issue token", err)
		return
	}

	writeJSON(w, http.StatusOK, map[string]any{
		"token":      token,
		"token_type": "Bearer",
		"expires_in": int(s.cfg.JWTTTL.Seconds()),
		"expires_at": exp.UTC(),
		"user":       s.userJSON(user.ID, user.Email, user.EngineUserID),
	})
}

func (s *Server) handleMe(w http.ResponseWriter, r *http.Request) {
	user := userFromContext(r.Context())
	if user == nil {
		writeError(w, http.StatusUnauthorized, "unauthorized", "missing or invalid token")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"user": s.userJSON(user.ID, user.Email, user.EngineUserID),
	})
}

func (s *Server) userJSON(id uuid.UUID, email, engineUID string) map[string]any {
	return map[string]any{
		"id":             id.String(),
		"email":          email,
		"engine_user_id": engineUID,
	}
}

func (s *Server) internalError(w http.ResponseWriter, action string, err error) {
	log.Printf("%s: %v", action, err)
	writeError(w, http.StatusInternalServerError, "internal_error", "internal error")
}

func validEmail(email string) bool {
	if len(email) > 254 || len(email) < 3 {
		return false
	}
	at := -1
	for i := 0; i < len(email); i++ {
		if email[i] == '@' {
			at = i
			break
		}
	}
	return at > 0 && at < len(email)-1
}

// grpcToHTTP maps engine gRPC status codes to HTTP status codes.
func grpcToHTTP(err error) (int, string) {
	switch status.Code(err) {
	case codes.NotFound:
		return http.StatusNotFound, "not_found"
	case codes.InvalidArgument:
		return http.StatusBadRequest, "invalid_argument"
	case codes.FailedPrecondition:
		return http.StatusConflict, "failed_precondition"
	case codes.AlreadyExists:
		return http.StatusConflict, "already_exists"
	case codes.Unauthenticated:
		return http.StatusUnauthorized, "unauthenticated"
	default:
		return http.StatusInternalServerError, "internal_error"
	}
}

func grpcMessage(err error) string {
	if msg := status.Convert(err).Message(); msg != "" {
		return msg
	}
	return fmt.Sprintf("engine error: %v", err)
}
