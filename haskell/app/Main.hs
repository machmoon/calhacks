-- | wright-oracle: read a JSON case (object) or array of cases on stdin,
-- write the evaluated case(s) as JSON on stdout. See Oracle.Cases.
module Main (main) where

import Oracle.Cases (evalCase)
import Oracle.Json
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)

main :: IO ()
main = do
  input <- getContents
  case parseJson input of
    Left e -> hPutStrLn stderr ("wright-oracle: " ++ e) >> exitFailure
    Right (JArr cs) -> putStrLn (render (JArr (map evalCase cs)))
    Right c -> putStrLn (render (evalCase c))
