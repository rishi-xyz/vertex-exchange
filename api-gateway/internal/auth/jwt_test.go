package auth

import (
	"strings"
	"testing"
	"time"
)

func TestIssueParseRoundtrip(t *testing.T) {
	m := NewManager("secret", time.Hour)
	token, exp, err := m.Issue("user-1", "engine-1")
	if err != nil {
		t.Fatalf("Issue: %v", err)
	}
	if exp.Before(time.Now()) {
		t.Fatal("expiry in the past")
	}
	claims, err := m.Parse(token)
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if claims.Subject != "user-1" {
		t.Errorf("subject = %q, want user-1", claims.Subject)
	}
	if claims.EngineUID != "engine-1" {
		t.Errorf("engine uid = %q, want engine-1", claims.EngineUID)
	}
}

func TestParseWrongSecret(t *testing.T) {
	token, _, _ := NewManager("secret-a", time.Hour).Issue("u", "e")
	if _, err := NewManager("secret-b", time.Hour).Parse(token); err == nil {
		t.Fatal("expected error for wrong secret")
	}
}

func TestParseExpired(t *testing.T) {
	m := NewManager("secret", -time.Minute)
	token, _, err := m.Issue("u", "e")
	if err != nil {
		t.Fatalf("Issue: %v", err)
	}
	if _, err := m.Parse(token); err == nil {
		t.Fatal("expected error for expired token")
	}
}

func TestParseTampered(t *testing.T) {
	token, _, _ := NewManager("secret", time.Hour).Issue("u", "e")
	parts := strings.Split(token, ".")
	if len(parts) != 3 {
		t.Fatalf("malformed token: %q", token)
	}
	payload := []byte(parts[1])
	if payload[0] == 'A' {
		payload[0] = 'B'
	} else {
		payload[0] = 'A'
	}
	tampered := parts[0] + "." + string(payload) + "." + parts[2]
	if _, err := NewManager("secret", time.Hour).Parse(tampered); err == nil {
		t.Fatal("expected error for tampered token")
	}
}

func TestCheckPassword(t *testing.T) {
	hash, err := HashPassword("hunter2")
	if err != nil {
		t.Fatalf("HashPassword: %v", err)
	}
	if !CheckPassword(hash, "hunter2") {
		t.Fatal("correct password rejected")
	}
	if CheckPassword(hash, "wrong") {
		t.Fatal("wrong password accepted")
	}
}
