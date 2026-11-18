# Command Program expression grammar

Spec node `AUTO.PROG.EXPR.GRAMMAR`. Project-authored, CC-BY-4.0. Implemented by module `expr::syntax` of crate `fid-program` (`parse_expr`, `print_expr`); an independent Python reference parser lives in `fixtures/program/expr/make_cases.py`.

Expressions are the source strings that Command Programs store in `expr` fields (`AUTO.PROG.FMT`). They steer programs; they never change state. The grammar has no assignment, declaration, loop, I/O or command-naming construct: `=` alone, `:`, `@` and `!` alone are not tokens, and a call can only name a function of the fixed library (`AUTO.PROG.EXPR.TYPES` resolves names, `AUTO.PROG.EXPR.EVAL` implements the functions).

## Lexical grammar

Source text is UTF-8. Whitespace (space, tab, LF, CR) separates tokens and is otherwise ignored.

```ebnf
digit      = "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" ;
letter     = "A" | ... | "Z" | "a" | ... | "z" ;
hex        = digit | "A" | ... | "F" | "a" | ... | "f" ;

int        = digit , { digit } ;                       (* value at most 2^63 - 1 *)
decimal    = digit , { digit } , "." , digit , { digit } ;
quantity   = ( decimal | int ) , "[" , unit-char , { unit-char } , "]" ;   (* no space before "[" *)
unit-char  = letter | digit | "_" | "." | "/" | "^" | "*" | "-" | "%"
           | "µ" (* U+00B5 *) | "μ" (* U+03BC *) | "°" (* U+00B0 *) | "Ω" (* U+03A9 *) ;
string     = '"' , { string-char | escape } , '"' ;
string-char = ? any Unicode scalar value except '"', "\", U+0000 to U+001F and U+007F ? ;
escape     = "\" , ( '"' | "\" | "n" | "r" | "t" | "u{" , hex , [ hex ] , [ hex ] , [ hex ] , [ hex ] , [ hex ] , "}" ) ;
                                                        (* \u{...} must name a Unicode scalar value *)
word       = ( letter | "_" ) , { letter | digit | "_" } ;   (* at most 64 bytes *)
keyword    = "and" | "else" | "false" | "if" | "not" | "or" | "then" | "true" ;
name       = word - keyword ;
punct      = "(" | ")" | "[" | "]" | "." | "," | "+" | "-" | "*" | "/" | "%"
           | "<" | "<=" | ">" | ">=" | "==" | "!=" ;
```

Tokens are formed longest-first (`<=` before `<`). A number followed directly by `[` is a quantity: the unit bracket is part of the token, so `2[mm]` is one quantity while `2 [0]` is an index. A `.` after digits starts a decimal only when a digit follows it, so `2.x` is field access on the integer 2. Decimal and quantity number text is kept verbatim (`12.700` stays `12.700`). Unit symbols are not checked against the unit catalogue here.

## Syntax

```ebnf
expr     = cond | or-expr ;
cond     = "if" , expr , "then" , expr , "else" , expr ;
or-expr  = and-expr , { "or" , and-expr } ;
and-expr = cmp-expr , { "and" , cmp-expr } ;
cmp-expr = add-expr , { ( "<" | "<=" | ">" | ">=" | "==" | "!=" ) , add-expr } ;
add-expr = mul-expr , { ( "+" | "-" ) , mul-expr } ;
mul-expr = unary , { ( "*" | "/" | "%" ) , unary } ;
unary    = ( "-" | "not" ) , unary | postfix ;
postfix  = primary , { "." , name | "[" , expr , "]" } ;
primary  = int | decimal | quantity | string | "true" | "false"
         | name , "(" , [ expr , { "," , expr } ] , ")"
         | name
         | "(" , expr , ")" ;
```

A conditional is not an operand: `1 + if a then 2 else 3` is a syntax error; write `1 + (if a then 2 else 3)`. Its `else` branch extends as far right as possible.

## Precedence and associativity

| Level | Operators | Associativity |
|---|---|---|
| 7 (tightest) | `.name`, `[index]`, `name(args)` | left (postfix) |
| 6 | unary `-`, `not` | right (prefix) |
| 5 | `*` `/` `%` | left |
| 4 | `+` `-` | left |
| 3 | `<` `<=` `>` `>=` `==` `!=` | left |
| 2 | `and` | left |
| 1 | `or` | left |
| 0 (loosest) | `if ... then ... else ...` | not an operand |

So `not a == b` is `(not a) == b`, `a < b < c` is `(a < b) < c` (the type checker decides whether that is meaningful), and `-a * b` is `(-a) * b`. Integer literals are never negative: `-5` is unary minus applied to 5.

## Syntax tree and spans

`Expr { kind, span }` where `span` is the half-open byte range `start..end` of the source. Literals and names span their token; a field, index or call spans from its base (or name) to its last character; unary and binary nodes span from their first to their last token; a conditional spans from `if` to the end of its `else` branch. A parenthesised expression has no node of its own, but its span is widened to include the parentheses, so the span of `(a + b) * c` starts at the `(`.

## Bounds

All input is treated as hostile. Parsing fails with `expr_too_large` and no tree when:

- the source is longer than 4 096 bytes (checked first); or
- a 1 025th token begins (checked while lexing; wins over syntax errors); or
- the nesting depth exceeds 64. Depth counts every edge of the syntax tree and every pair of parentheses: `x` has depth 0, `(x)` and `-x` depth 1, `f(a)` depth 1, `a + b + c` depth 2. Sixty-four nested parentheses around a name are accepted; sixty-five are not. Long left-associative chains count too (`a + a + ... ` with 66 operands is too deep), which keeps every later recursive pass over the tree within 64 levels.

The parser carries an explicit depth counter and refuses to descend past the bound, so its recursion is bounded by a small constant times 64.

## Errors

Every other failure is `expr_syntax` with `offset`, the byte offset of the first offending token. Errors are reported in source order: if the tokens before a lexical error already fail to parse, that earlier offset is reported. Where each error points:

| Situation | Offset |
|---|---|
| unexpected token (including `)` or `]` with no opener, a keyword where a name is needed) | start of that token |
| source ends too early (unclosed `(`, `[`, call, missing `else`, trailing operator) | source length |
| character that starts no token (`=` alone, `!` alone, `:`, `@`, `#`, non-ASCII outside a string) | that character |
| integer above 2^63 - 1, or a word longer than 64 bytes | start of the token |
| quantity unit empty, unclosed or with a character outside `unit-char` | the `[` |
| unclosed string | the opening `"` |
| invalid escape | its `\` |
| raw control character in a string | that character |

## Printing

`print_expr` writes the shortest source for a tree: parentheses only where precedence or left-associativity needs them, a space only where two tokens would merge (a name or keyword before a word character, or a number before a digit or an index `[`), integers without leading zeros, strings with the shortest escapes. For every tree `parse_expr` returns, parsing the printed text gives an equal tree (spans aside), and the printed text is never longer, never has more tokens and never nests deeper than the source it came from. The output favours shortness over looks: `( a+b ) *  -c` prints as `(a+b)*-c`, and `x > 1 and not y` as `x>1and not y` (a number followed by a letter is two tokens).
