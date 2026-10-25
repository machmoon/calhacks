-- | A minimal JSON reader/writer on `base` only (ReadP), enough for the
-- oracle's case files. Kept dependency-free so the oracle builds offline
-- with the GHC boot packages.
module Oracle.Json
  ( J (..)
  , parseJson
  , render
  , field
  , num
  , str
  ) where

import Data.Char (isDigit, isHexDigit, isSpace, chr, ord)
import Data.List (intercalate)
import Numeric (readHex, showHex)
import Text.ParserCombinators.ReadP

data J = JNull | JBool Bool | JNum Double | JStr String | JArr [J] | JObj [(String, J)]
  deriving (Show, Eq)

field :: String -> J -> Maybe J
field k (JObj kvs) = lookup k kvs
field _ _ = Nothing

num :: String -> J -> Maybe Double
num k o = case field k o of
  Just (JNum x) -> Just x
  Just (JBool b) -> Just (if b then 1 else 0)
  _ -> Nothing

str :: String -> J -> Maybe String
str k o = case field k o of
  Just (JStr s) -> Just s
  _ -> Nothing

parseJson :: String -> Either String J
parseJson s = case [v | (v, rest) <- readP_to_S (ws *> value <* ws) s, null rest] of
  (v : _) -> Right v
  [] -> Left "invalid JSON"

ws :: ReadP ()
ws = skipSpaces

tok :: String -> ReadP String
tok t = string t <* ws

value :: ReadP J
value = (obj +++ arr +++ (JStr <$> strLit) +++ number +++ lit) <* ws
  where
    lit = (JNull <$ string "null") +++ (JBool True <$ string "true") +++ (JBool False <$ string "false")

obj :: ReadP J
obj = JObj <$> between (tok "{") (char '}') (sepBy kv (tok ","))
  where kv = (,) <$> (strLit <* ws <* tok ":") <*> value

arr :: ReadP J
arr = JArr <$> between (tok "[") (char ']') (sepBy value (tok ","))

strLit :: ReadP String
strLit = char '"' *> many ch <* char '"'
  where
    ch = (satisfy (\c -> c /= '"' && c /= '\\')) +++ (char '\\' *> esc)
    esc = choice
      [ '"' <$ char '"', '\\' <$ char '\\', '/' <$ char '/', '\b' <$ char 'b'
      , '\f' <$ char 'f', '\n' <$ char 'n', '\r' <$ char 'r', '\t' <$ char 't'
      , char 'u' *> (hex4 <$> count 4 (satisfy isHexDigit)) ]
    hex4 h = case readHex h of
      [(n, "")] -> chr n
      _ -> '?'

number :: ReadP J
number = do
  sgn <- option "" (string "-")
  int <- munch1 isDigit
  frac <- option "" ((:) <$> char '.' <*> munch1 isDigit)
  ex <- option "" $ do
    e <- satisfy (`elem` "eE")
    s <- option "" (string "+" +++ string "-")
    d <- munch1 isDigit
    return (e : (if s == "+" then "" else s) ++ d)
  let frac' = if null frac then ".0" else frac
  return (JNum (read (sgn ++ int ++ frac' ++ ex)))

render :: J -> String
render JNull = "null"
render (JBool b) = if b then "true" else "false"
render (JNum x)
  | isNaN x || isInfinite x = "null"
  | otherwise = show x
render (JStr s) = show' s
render (JArr xs) = "[" ++ intercalate "," (map render xs) ++ "]"
render (JObj kvs) = "{" ++ intercalate "," [show' k ++ ":" ++ render v | (k, v) <- kvs] ++ "}"

show' :: String -> String
show' s = "\"" ++ concatMap e s ++ "\""
  where
    e '"' = "\\\""
    e '\\' = "\\\\"
    e '\n' = "\\n"
    e c | ord c < 0x20 || isSpace c && c /= ' ' = "\\u" ++ pad (showHex (ord c) "")
        | otherwise = [c]
    pad h = replicate (4 - length h) '0' ++ h
