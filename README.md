# code4rena-scraper
Scrapes for code4rena contracts and builds them

This repo contains 2 branches, the default master branch implements the scrape and compile source contracts in memory method. However tracing the imported files and compiling them started to become too much work. The second branch "clone-repo-method" is probably the better of the 2 methods, where we clone the repo and all of it's dependencies to local. Then we compile. This works except the final piece which is to get the bytecode from the contracts. The function to get the bytecode from the compiler output is in there, just need to add the filename to index into the compiler output.
