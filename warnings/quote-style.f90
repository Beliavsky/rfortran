! Warning:    quote-style
! Enabled by: -Weverything
! A CHARACTER literal in single quotes, when double quotes are preferred
! (the default), or in double quotes with -Wquote-style=single. One kind of
! quote throughout is consistent; the warning rewrites the statement.
program quote_style
implicit none
print *, 'single'          ! expect: quote-style
print *, "double"
print *, "it's"
end program quote_style
