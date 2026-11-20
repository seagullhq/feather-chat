package control

import (
	"errors"
	"strings"
	"unicode/utf8"
)

// messageContent applies the bounds shared by the conversation_messages check
// constraint: non-blank and at most 4000 runes. The label only shapes the error
// text, so each endpoint still tells the caller which content was rejected.
func messageContent(content, label string) (string, error) {
	if strings.TrimSpace(content) == "" {
		return "", errors.New(label + " message content required")
	}
	if n := utf8.RuneCountInString(content); n > 4000 {
		return "", errors.New(label + " message must be at most 4000 characters")
	}
	return content, nil
}
