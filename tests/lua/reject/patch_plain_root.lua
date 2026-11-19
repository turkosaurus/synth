-- expect: a patch must return synth.instrument({…}) or synth.sequence({…}), got a plain table
-- a patch returns a constructed instrument or sequence, not a plain table
return {}
