/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

package ebnf

import (
	"encoding/json"
	"errors"
	"io/fs"
	"os"
	"strings"
	"testing"

	bnf "github.com/tabnas/bnf/go"
)

// The parts a host sees are the package's embedded copies, written by
// `npm run embed` in ts/; they must be the repository's files.
func TestTranslationParts(t *testing.T) {
	inCheckout(t)
	parts := Translate()
	if parts == nil {
		t.Fatal("Translate returned nil")
	}
	manifest, err := os.ReadFile("../tabnas.plugin.json")
	if err != nil {
		t.Fatal(err)
	}
	if parts.Manifest != string(manifest) {
		t.Fatal("embedded manifest differs from tabnas.plugin.json: run npm run embed in ts")
	}
	if parts.Lift != nil {
		t.Fatal("ebnf has no lift")
	}
	if parts.Render == nil || parts.Render.Entry != "ebnf-render" {
		t.Fatalf("render entry is %#v", parts.Render)
	}
	render, err := os.ReadFile("../alchemy/render.alc")
	if err != nil {
		t.Fatal(err)
	}
	if parts.Render.Source != string(render) {
		t.Fatal("embedded render differs from alchemy/render.alc: run npm run embed in ts")
	}
	for _, p := range []struct {
		part  *TranslationPart
		entry string
		file  string
	}{
		{parts.Transduce, "ebnf-transduce", "../alchemy/transduce.alc"},
		{parts.Spec, "ebnf-render-spec", "../alchemy/render-spec.alc"},
	} {
		if p.part == nil || p.part.Entry != p.entry {
			t.Fatalf("%s's part is %#v", p.entry, p.part)
		}
		text, err := os.ReadFile(p.file)
		if err != nil {
			t.Fatal(err)
		}
		if p.part.Source != string(text) {
			t.Fatalf("embedded %s differs from %s: run npm run embed in ts", p.entry, p.file)
		}
	}
}

// An embed takes a plain tree into a format's own schema. A grammar spec
// is no plain tree's shape, so the manifest names no embed and the
// package carries none; a manifest that named one would be held to its
// file here, as the render is above.
func TestTranslationEmbed(t *testing.T) {
	inCheckout(t)
	manifest, err := os.ReadFile("../tabnas.plugin.json")
	if err != nil {
		t.Fatal(err)
	}
	var spec struct {
		Translate struct {
			Embed *string `json:"embed"`
		} `json:"translate"`
	}
	if err := json.Unmarshal(manifest, &spec); err != nil {
		t.Fatal(err)
	}
	parts := Translate()
	if spec.Translate.Embed == nil {
		if parts.Embed != nil {
			t.Fatalf("the manifest names no embed, and Translate carries %#v", parts.Embed)
		}
		return
	}
	if parts.Embed == nil || parts.Embed.Entry != "ebnf-embed" {
		t.Fatalf("embed entry is %#v", parts.Embed)
	}
	embed, err := os.ReadFile("../" + *spec.Translate.Embed)
	if err != nil {
		t.Fatal(err)
	}
	if parts.Embed.Source != string(embed) {
		t.Fatalf("embedded embed differs from %s", *spec.Translate.Embed)
	}
}

// The tree is the grammar's rules, an object, of the schema the three
// BNF-family notations share, streamed by the part, so the manifest names
// no lift and nothing read whole; the compiled spec's route is spec, of
// the schema grammar-spec.
func TestTranslationSchema(t *testing.T) {
	type route struct {
		Reads     string   `json:"reads"`
		Writes    string   `json:"writes"`
		Root      string   `json:"root"`
		Schema    string   `json:"schema"`
		Lift      *string  `json:"lift"`
		Transduce *string  `json:"transduce"`
		Whole     *string  `json:"whole"`
		Loss      []string `json:"loss"`
	}
	var spec struct {
		LanguageID string `json:"languageId"`
		Translate  struct {
			route
			Spec route `json:"spec"`
		} `json:"translate"`
	}
	if err := json.Unmarshal([]byte(Translate().Manifest), &spec); err != nil {
		t.Fatal(err)
	}
	tr := spec.Translate
	if spec.LanguageID != "ebnf" || tr.Reads != "tree" || tr.Writes != "tree" || tr.Root != "object" ||
		tr.Schema != "grammar-rules" || tr.Lift != nil || tr.Whole != nil ||
		tr.Transduce == nil || *tr.Transduce != "alchemy/transduce.alc" {
		t.Fatalf("translate is %+v", tr.route)
	}
	if tr.Spec.Schema != "grammar-spec" || tr.Spec.Root != "object" {
		t.Fatalf("translate.spec is %+v", tr.Spec)
	}
	for _, loss := range [][]string{tr.Loss, tr.Spec.Loss} {
		if len(loss) == 0 {
			t.Fatal("a loss list is empty")
		}
		for _, line := range loss {
			if line == "" || !strings.HasSuffix(line, ".") || strings.ToUpper(line[:1]) != line[:1] {
				t.Fatalf("%q is not a sentence", line)
			}
		}
	}
}

// The part and the renders are libraries a host links with its own
// program: every definition is named for EBNF, each defines its entry
// point, and the two renders, which a host may link together, share no
// name.
func TestTranslationRenderIsALibrary(t *testing.T) {
	defs := func(source string) map[string]bool {
		names := map[string]bool{}
		for _, line := range strings.Split(source, "\n") {
			if !strings.HasPrefix(line, "def ") {
				continue
			}
			name := strings.Fields(strings.TrimPrefix(line, "def "))[0]
			if !strings.HasPrefix(name, "ebnf-") {
				t.Fatalf("%s is not named for ebnf", name)
			}
			names[name] = true
		}
		return names
	}
	parts := Translate()
	for _, part := range []*TranslationPart{parts.Transduce, parts.Render, parts.Spec} {
		if !defs(part.Source)[part.Entry] {
			t.Fatalf("a part defines no %s", part.Entry)
		}
	}
	render := defs(parts.Render.Source)
	for name := range defs(parts.Spec.Source) {
		if render[name] {
			t.Fatalf("%s is defined in both renders", name)
		}
	}
}

// What a host that takes the grammar-spec route reads an EBNF document
// as: the pure-data grammar spec of the spec Ebnf converts with Builtins on, which bnf.ToPureSpec and
// bnf.ToJsonic write as strict JSON (its keys in name order, where the
// TypeScript and Rust compilers write them in the order they emit them),
// valid JSON whose root is an object holding the rules and the options.
func TestTranslationReadsTheGrammarSpec(t *testing.T) {
	spec, err := Ebnf("Doc ::= Greet\nGreet ::= \"hi\" | \"hello\"", &ConvertOptions{Builtins: true})
	if err != nil {
		t.Fatal(err)
	}
	pure, err := bnf.ToPureSpec(spec)
	if err != nil {
		t.Fatal(err)
	}
	var tree map[string]any
	if err := json.Unmarshal([]byte(bnf.ToJsonic(pure, true, 2)), &tree); err != nil {
		t.Fatalf("the grammar spec is not JSON: %v", err)
	}
	rules, ok := tree["rule"].(map[string]any)
	if !ok || rules["Doc"] == nil || rules["Greet"] == nil || rules["__start__"] == nil {
		t.Fatalf("the grammar spec's rules are %v", tree["rule"])
	}
	if _, ok := tree["options"].(map[string]any); !ok {
		t.Fatalf("the grammar spec has no options: %v", tree)
	}
}

// Every call hands back parts of its own: a caller that changes what it
// was given, the parts or a part they point to, changes nothing the next
// caller reads, and callers on several goroutines share nothing to race on.
func TestTranslateReturnsACopy(t *testing.T) {
	first := Translate()
	want := *Translate().Render
	first.Manifest = ""
	if first.Render != nil {
		first.Render.Entry = ""
		first.Render.Source = ""
	}
	first.Lift, first.Embed, first.Render = nil, nil, nil
	second := Translate()
	if second.Manifest == "" {
		t.Fatal("a change to one call's manifest reached the next call")
	}
	if second.Render == nil || *second.Render != want {
		t.Fatalf("a change to one call's render reached the next call: %#v", second.Render)
	}
}

// inCheckout skips a test that holds the embedded copies to the
// repository's own files when it runs where those files are not, as from
// the module cache, whose zip holds the go/ module alone. In a checkout,
// a missing file still fails the test that reads it.
func inCheckout(t *testing.T) {
	t.Helper()
	if _, err := os.Stat("../ts/package.json"); errors.Is(err, fs.ErrNotExist) {
		t.Skip("not in a checkout of the repository: the module cache holds the go/ module alone")
	}
}
