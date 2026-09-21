// Generates rs/tests/oracle/ebnf-ir.json: for each EBNF source, what the
// canonical TypeScript front-end answers -- the IR, or the rejection.
'use strict'
const Fs = require('node:fs')
const Path = require('node:path')

const REPO = '/home/user/ebnf'
const { parseEbnf, ebnfConvert } = require(Path.join(REPO, 'ts', 'dist', 'ebnf.js'))

const fixtures = ['expr.ebnf', 'iso-style.ebnf', 'json-subset.ebnf', 'name.ebnf']
  .map((n) => Fs.readFileSync(Path.join(REPO, 'ts', 'test', 'grammar', n)).toString())

const SOURCES = [
  // --- IR shape ---
  'A ::= "x" B\nB ::= "y"',
  'A ::= "x" | "y" | "z"',
  'A ::= "x"? "y"* "z"+',
  'A ::= ( "x" | "y" ) "z"',
  'A ::= ( "x" )*?',
  'A ::= ( "x" )?*+',
  'A ::= [a-z]',
  'A ::= [^<&]',
  'A ::= [#x20-#x7E]\nB ::= [#x9#xA#xD]',
  'A ::= [-+]\nB ::= [a-]',
  'A ::= [#x10000-#x10FFFF]',
  'A ::= [abc]',
  'A ::= [^a-zA-Z0-9]',
  'A ::= [\\\\]',
  'A ::= [.*+?]',
  'A ::= #x41',
  'A ::= #xD7FF\nB ::= #xd7ff',
  "A ::= 'a\"b' | \"c'd\"",
  'A ::= "\\n"',
  'A ::= "]"',
  // --- ISO spellings ---
  'a = "x" ;\nb = "y" ;',
  'a ::= "x"\nb ::= "y" ;',
  'a ::= "x" , "y"',
  'a ::= "x" "y"',
  '(* leading *) g = "a" (* inline *) "b"',
  '(* one\ntwo\nthree *)\na ::= "x"\nb ::= { "y" }',
  '/* leading */ G ::= "a" /* inline */ "b" /* trailing */',
  'greet = "hi" | "hello"',
  // --- names ---
  '_x ::= "a"\nx.y ::= "b"\nx-y ::= "c"\nChar32 ::= "d"\nA ::= _x x.y x-y Char32',
  'true ::= "t"\nfalse ::= "f"\nnull ::= "n"\nA ::= true | false | null',
  // --- built-in tokens ---
  'Pair ::= "{" Key ":" Val "}"\nKey ::= TX\nVal ::= NR | ST',
  // --- left recursion ---
  'E ::= E "+" T | T\nT ::= "1"',
  'Expr ::= Expr "+" Term | Term\nTerm ::= NR',
  // --- nullable, accepted ---
  'A ::= "x"? | "y"',
  'A ::= ("x"? | "y") "z"',
  'S ::= A\nA ::= B\nB ::= "a"*',
  'S ::= A B\nA ::= "x"?\nB ::= "y"?',
  'S ::= A "x" | B "y"\nA ::= "a" A | "a"\nB ::= "a" B | "a"',
  // --- spans ---
  ['doc ::= item', 'item ::= "hi" | ref | (alt | two)', 'ref ::= [a-z]',
    'alt ::= #x41', 'two ::= "z"'].join('\n'),
  // --- fixtures ---
  ...fixtures,
  // --- rejections ---
  'Foo! ::= "x"',
  'A ::= B - C\nB ::= "b"\nC ::= "c"',
  'A ::= B -C\nB ::= "b"',
  'A ::= ? anything at all ?',
  'A ::= { "x" }',
  'A ::= } "x"',
  'A ::= [a-z',
  'A ::= *"x"',
  'A ::= +"x"',
  'A ::= ( "x"',
  'A ::= ""',
  "A ::= ''",
  'A ::= []',
  'A ::= [z-a]',
  'A ::= #x110000',
  'A ::= [#x0-#x110000]',
  'A ::= [#x]',
  'A ::= #x',
  'A ::= #X41',
  'A ::= [#X20-#X7E]',
  'A ::= "x"\nA ::= "y"',
  'A ::= "x"? | "y"?',
  'A ::= ("x"? | "y"?) "y"',
  'A ::= (("x"? | "y"?) "z") "w"',
  'A ::= | "x"',
  'A ::= "x" |',
  'A = ;',
  'A ::= ()',
  'A = , "x";',
  'A ::= "x" , ',
  'A ::= B',
  'A ::= A "x"',
  '',
  '/* nothing but a comment */',
  '   \n\t\n',
  'A ::= "x" ]',
  'A ::= "x" ) "y"',
  'A',
  'A ::=',
  '::= "x"',
  '"x" ::= "y"',
  'A ::= "x" B C\nB ::= "b"\nC ::= "c"',
]

const out = []
for (const src of SOURCES) {
  let entry = { src }
  try {
    entry.ir = JSON.parse(JSON.stringify(parseEbnf(src)))
  } catch (e) {
    entry.error = String(e.message)
    if (null != e.line) entry.line = e.line
    if (null != e.column) entry.column = e.column
  }
  if (undefined === entry.error) {
    try {
      const spec = ebnfConvert(src)
      entry.rules = Object.keys(spec.rule).sort()
      entry.emptyOk = spec.options.lex.empty
    } catch (e) {
      entry.compileError = String(e.message)
    }
  }
  out.push(entry)
}

Fs.writeFileSync(
  Path.join(REPO, 'rs', 'tests', 'oracle', 'ebnf-ir.json'),
  JSON.stringify(out, null, 1) + '\n')
console.log('wrote', out.length, 'entries')
