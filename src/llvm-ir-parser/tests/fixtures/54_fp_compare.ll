; FP comparison instructions — issue #291
; Tests fcmp with ordered and unordered predicates.
define i1 @fcmp_olt(double %a, double %b) {
entry:
  %r = fcmp olt double %a, %b
  ret i1 %r
}
define i1 @fcmp_oeq(double %a, double %b) {
entry:
  %r = fcmp oeq double %a, %b
  ret i1 %r
}
define i1 @fcmp_ogt(double %a, double %b) {
entry:
  %r = fcmp ogt double %a, %b
  ret i1 %r
}
define i1 @fcmp_une(float %a, float %b) {
entry:
  %r = fcmp une float %a, %b
  ret i1 %r
}
