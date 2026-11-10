// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

package ebnf

// A parse from a reader, a window of the source at a time (admin ADR-30;
// tabnas/parser's doc/chunked-source.md, "Proving it"), against the same
// text parsed whole. Make, the tree parser, declares Lex.Window: the
// meta-grammar's match tokens and its ISO comments are read behind ebnf-reach, a matcher
// that reads the window on until the byte that decides where their token
// ends lies inside it, and every other token is the engine's.
//
// Every row of the shared fixture, every grammar of ts/test/grammar and the
// source of every case of the oracle (rs/tests/oracle/ebnf-ir.json) is parsed whole and from a reader
// that hands its bytes over in pieces of 1, 3 and 64 bytes. The engine's
// window sizes are fixed in Go (only its own tests can make them a few
// bytes), and the window reaches 64 KiB past the cursor before each token
// is cut, so the edge is proved with long documents: their tokens of every
// kind straddle the reads, and the long ones, longer than the window, run
// past its end, in pieces of 61, 4096 and 65536 bytes. The value, the
// tokens the lexer gives, the rule passes, and an error's code, position,
// row, column and token must be the same. An error's excerpt is not
// compared: a parse from a reader shows a long line only as far back as the
// window reaches (tabnas/parser's DIVERGENCE.md).
//
// ts/test/window.test.js and rs/tests/window_test.rs hold the TypeScript
// and Rust ports to the same, under windows of a few units too.

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"strings"
	"testing"
	"time"

	tabnas "github.com/tabnas/parser/go"
)

// pieces is a reader that hands its bytes over at most size at a time.
type pieces struct {
	bytes []byte
	at    int
	size  int
}

func (p *pieces) Read(buf []byte) (int, error) {
	if p.at >= len(p.bytes) {
		return 0, io.EOF
	}
	n := len(p.bytes) - p.at
	if n > p.size {
		n = p.size
	}
	if n > len(buf) {
		n = len(buf)
	}
	copy(buf, p.bytes[p.at:p.at+n])
	p.at += n
	return n, nil
}

// readSizes are the pieces a reader hands a fixture row or a grammar over
// in, bigSizes a grammar's longer than 16 KiB, and longSizes a long
// document's: the Go engine copies its window at each read
// (tabnas/parser#321), so a read of a few bytes costs the window's length,
// and a long document read a byte at a time takes seconds where its parse
// takes milliseconds.
var (
	readSizes = []int{1, 3, 64}
	bigSizes  = []int{64, 4096}
	longSizes = []int{61, 4096, 65536}
)

// windowLog gathers the tokens the lexer gives and the rule passes.
type windowLog struct {
	tokens []string
	rules  []string
}

// logged is the tree parser, its tokens and rule passes subscribed to log.
func logged(log *windowLog) *tabnas.Tabnas {
	j := Make()
	j.Sub(func(tkn *tabnas.Token, _ *tabnas.Rule, _ *tabnas.Context) {
		val, _ := json.Marshal(tkn.Val)
		log.tokens = append(log.tokens, fmt.Sprintf("%s %q %s %d %d:%d",
			tkn.Name, tkn.Src, val, tkn.SI, tkn.RI, tkn.CI))
	}, nil)
	j.SubRuleDone(func(r *tabnas.Rule, _ *tabnas.Context, done tabnas.RuleDone) {
		log.rules = append(log.rules, fmt.Sprintf("%s %v %d", r.Name, done.State, r.D))
	})
	return j
}

// outcome is a parse's value as JSON, or its error's code, position, row,
// column and token.
func outcome(value any, err error) string {
	if err != nil {
		var te *tabnas.TabnasError
		if errors.As(err, &te) {
			return fmt.Sprintf("err %s at %d %d:%d %q", te.Code, te.Pos, te.Row, te.Col, te.Src)
		}
		return "fail " + err.Error()
	}
	text, err := json.Marshal(value)
	if err != nil {
		return "unmarshallable " + err.Error()
	}
	return "ok " + string(text)
}

// differential parses text whole and from a reader in each of sizes, and
// fails t where the two differ. It returns the count of reader parses
// compared.
func differential(t *testing.T, j *tabnas.Tabnas, log *windowLog, at, text string, sizes []int) int {
	t.Helper()
	log.tokens, log.rules = nil, nil
	value, err := j.Parse(text)
	whole, tokens, rules := outcome(value, err), log.tokens, log.rules
	for _, size := range sizes {
		log.tokens, log.rules = nil, nil
		value, err := j.ParseReader(&pieces{bytes: []byte(text), size: size})
		if read := outcome(value, err); read != whole {
			t.Fatalf("%s in pieces of %d:\nread  %.300s\nwhole %.300s", at, size, read, whole)
		}
		if !reflect.DeepEqual(log.tokens, tokens) {
			t.Fatalf("%s in pieces of %d: the tokens differ\nread  %.400v\nwhole %.400v", at, size, log.tokens, tokens)
		}
		if !reflect.DeepEqual(log.rules, rules) {
			t.Fatalf("%s in pieces of %d: the rule passes differ", at, size)
		}
	}
	return len(sizes)
}

// longDocument is a grammar of every kind of token, the long ones longer
// than the engine's 64 KiB window, and characters past ASCII among them, so
// that tokens of every kind straddle the window's edges as a reader hands
// the text over. The same document as long_document in
// rs/tests/window_test.rs and longDocument in ts/test/window.test.js.
func longDocument() string {
	var b strings.Builder
	b.WriteString("/* " + strings.Repeat("a comment past the window, \u00e9 \U0001D11E * / ", 2000) + "*/\n")
	b.WriteString("(* " + strings.Repeat("an ISO comment past the window, \u00fc ( * \n", 1800) + "*)\n")
	b.WriteString("long ::= \"" + strings.Repeat("a string past the window \u00e7 ' ", 2800) + "\"\n")
	b.WriteString("single ::= '" + strings.Repeat("a quoted string past the window \" ", 2400) + "'\n")
	b.WriteString("klass ::= [" + strings.Repeat("a-z0-9_#x41#xE9-#xFF\u00f1", 2600) + "]\n")
	b.WriteString("named ::= " + strings.Repeat("n", 70000) + "\n")
	b.WriteString(strings.Repeat("n", 70000) + " ::= #x41\n")
	for i := 0; i < 4000; i++ {
		hex := fmt.Sprintf("%X", 0x41+(i%26))
		fmt.Fprintf(&b, "r%d ::= #x%s | [a-z#x%s] | \"s%d\" | 'q%d' | (r%d | \"x\")* | n%d+ | \"y\"? /* c%d \u00e9 */\n",
			i, hex, hex, i, i, i, i, i)
		fmt.Fprintf(&b, "s%d = \"a\" , \"b\" , ( \"c\" | \"d\" ) ; (* iso %d *)\n", i, i)
		fmt.Fprintf(&b, "t%d ::= [^\"#x0A] ( u%d v%d )+\n", i, i, i)
	}
	b.WriteString("end ::= \"done\"\n")
	return b.String()
}

func TestMakeReadsAReaderAWindowAtATime(t *testing.T) {
	lex := Make().Options().Lex
	if lex == nil || lex.Window == nil || !*lex.Window {
		t.Fatal("Make does not declare Lex.Window")
	}
}

func TestEveryFixtureRowParsesTheSameFromAReader(t *testing.T) {
	log := &windowLog{}
	j := logged(log)
	rows := treeRows(t)
	compared := 0
	for _, row := range rows {
		compared += differential(t, j, log, fmt.Sprintf("tree.tsv:%d", row.line), row.grammar, readSizes)
	}
	if len(rows) < 30 || compared != len(rows)*len(readSizes) {
		t.Fatalf("%d rows, %d reader parses", len(rows), compared)
	}
}

func TestEveryGrammarAndOracleSourceParsesTheSameFromAReader(t *testing.T) {
	type document struct{ at, text string }
	var documents []document
	paths, _ := filepath.Glob(filepath.Join("..", "ts", "test", "grammar", "*.ebnf"))
	sort.Strings(paths)
	for _, path := range paths {
		raw, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		documents = append(documents, document{path, string(raw)})
	}
	raw, err := os.ReadFile(filepath.Join("..", "rs", "tests", "oracle", "ebnf-ir.json"))
	if err != nil {
		t.Fatal(err)
	}
	var cases []struct {
		Src string `json:"src"`
	}
	if err := json.Unmarshal(raw, &cases); err != nil {
		t.Fatal(err)
	}
	for i, c := range cases {
		documents = append(documents, document{fmt.Sprintf("oracle case %d", i), c.Src})
	}
	if len(documents) < 60 {
		t.Fatalf("%d documents", len(documents))
	}
	log := &windowLog{}
	j := logged(log)
	for _, d := range documents {
		sizes := readSizes
		if len(d.text) > 16*1024 {
			sizes = bigSizes
		}
		differential(t, j, log, d.at, d.text, sizes)
	}
}

func TestALongDocumentParsesTheSameFromAReader(t *testing.T) {
	text := longDocument()
	if len(text) < 512*1024 {
		t.Fatalf("%d bytes", len(text))
	}
	tree, err := Make().Parse(text)
	if err != nil {
		t.Fatal(err)
	}
	if rules, _ := tree.(*tabnas.OrderedMap).Get("rules"); len(rules.([]any)) != 12006 {
		t.Fatalf("%d rules", len(rules.([]any)))
	}
	log := &windowLog{}
	differential(t, logged(log), log, "the long document", text, longSizes)
}

func TestALongDocumentFailsTheSameFromAReader(t *testing.T) {
	// A token that runs past the engine's window and is then refused, or
	// never ends: the error is the whole text's, code, place and token.
	pad := "/* " + strings.Repeat("padding \u00fc ", 9000) + "*/\na ::= \"x\"\n"
	bad := []string{
		pad + "b ::= [" + strings.Repeat("a-z", 30000) + "\nc ::= \"y\"\n",
		pad + "b ::= \"" + strings.Repeat("unclosed string ", 6000) + "\n",
		pad + "b ::= ( c | " + strings.Repeat("d ", 40000) + "\n",
		pad + "(* " + strings.Repeat("an unclosed ISO comment ", 4000) + "\n",
		pad + "b ::= c - " + strings.Repeat("d ", 40000) + "\n",
		pad + "b ::= [z-a] " + strings.Repeat("e ", 40000) + "\n",
		pad + "b ::= " + strings.Repeat("f ", 40000) + "#x" + strings.Repeat("4", 70000) + "\n",
		pad + strings.Repeat("j", 70000) + " ::= \"g\" ]\n",
	}
	log := &windowLog{}
	j := logged(log)
	for index, text := range bad {
		if _, err := j.Parse(text); err == nil {
			t.Fatalf("long error document %d parses", index)
		}
		differential(t, j, log, fmt.Sprintf("long error document %d", index), text, longSizes)
	}
}

func TestALongDocumentParsesFromAReaderInTimeLinearInItsLength(t *testing.T) {
	// Ten times the rules, read through the engine's window in pieces of 4
	// KiB, take about ten times as long, never the square of it.
	j := Make()
	document := func(rules int) string {
		var b strings.Builder
		for i := 0; i < rules; i++ {
			fmt.Fprintf(&b, "r%d ::= [A-Z] | \"b%d\" | #x41 /* d */\n", i, i)
		}
		return b.String()
	}
	timed := func(text string) time.Duration {
		started := time.Now()
		if _, err := j.ParseReader(&pieces{bytes: []byte(text), size: 4096}); err != nil {
			t.Fatal(err)
		}
		return time.Since(started)
	}
	small, large := document(2000), document(20000)
	timed(small)
	one, ten := timed(small), timed(large)
	t.Logf("2000 rules %v, 20000 rules %v, %.1f times", one, ten, float64(ten)/float64(one))
	if ten > 30*one {
		t.Fatalf("2000 rules %v, 20000 rules %v", one, ten)
	}
}
