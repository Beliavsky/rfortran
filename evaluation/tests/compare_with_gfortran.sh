#!/bin/sh
# usage: run.sh file...   compares rfort vs gfortran output
R=/c/rust/public_domain/github/rfortran/target/release/rfort.exe
for f in "$@"; do
  echo "=== $f"
  rm -f g.exe r.exe *.mod *.rmod
  gfortran -w -o g.exe "$f" > gc.txt 2>&1 || { echo "gfortran compile fail:"; head -3 gc.txt; }
  [ -f g.exe ] && timeout 20 ./g.exe > g.out 2>&1 < /dev/null
  rm -f *.mod
  timeout 60 $R -o r.exe "$f" > rc.txt 2>&1 < /dev/null
  echo "rfort exit=$?"; head -8 rc.txt
  if [ -f r.exe ]; then
    timeout 20 ./r.exe > r.out 2>&1 < /dev/null
    echo "run exit=$?"
    if diff -wq g.out r.out > /dev/null; then echo "MATCH gfortran"; else echo "--gf:"; cat g.out; echo "--rf:"; cat r.out; fi
  fi
done
