//#region ../../wt-query-graph/web/node_modules/@traqula/core/dist/esm/lib/AstCoreFactory.js
var e = class {
	tracksSourceLocation;
	constructor(e = {}) {
		this.tracksSourceLocation = e.tracksSourceLocation ?? !0;
	}
	wrap(e, t) {
		return {
			val: e,
			loc: t
		};
	}
	isLocalized(e) {
		return typeof e == "object" && !!e && "loc" in e && typeof e.loc == "object" && e.loc !== null && "sourceLocationType" in e.loc;
	}
	sourceLocation(...e) {
		if (!this.tracksSourceLocation) return this.gen();
		let t = e.filter((e) => e !== void 0);
		if (t.length === 0) return this.sourceLocationNoMaterialize();
		let n = t.filter((e) => !this.isLocalized(e) || this.isSourceLocationSource(e.loc) || this.isSourceLocationStringReplace(e.loc) || this.isSourceLocationNodeReplace(e.loc));
		if (n.length === 0) return this.gen();
		let r = n.at(0), i = n.at(-1);
		return {
			sourceLocationType: "source",
			start: this.isLocalized(r) ? r.loc.start : r.startOffset,
			end: this.isLocalized(i) ? i.loc.end : i.endOffset + 1
		};
	}
	sourceLocationNoMaterialize() {
		return this.tracksSourceLocation ? { sourceLocationType: "noMaterialize" } : this.gen();
	}
	dematerialized(e) {
		return {
			...e,
			loc: this.sourceLocationNoMaterialize()
		};
	}
	safeObjectTransform(e, t) {
		return e && typeof e == "object" ? Array.isArray(e) ? e.map((e) => this.safeObjectTransform(e, t)) : t(e) : e;
	}
	forcedAutoGenTree(e) {
		let t = { ...e };
		for (let [e, n] of Object.entries(t)) t[e] = this.safeObjectTransform(n, (e) => this.forcedAutoGenTree(e));
		return this.isLocalized(t) && (t.loc = this.gen()), t;
	}
	forceMaterialized(e) {
		return this.isSourceLocationNoMaterialize(e.loc) ? this.forcedAutoGenTree(e) : { ...e };
	}
	isSourceLocation(e) {
		return "sourceLocationType" in e;
	}
	sourceLocationSource(e, t) {
		return {
			sourceLocationType: "source",
			start: e,
			end: t
		};
	}
	sourceLocationInlinedSource(e, t, n, r, i = 0, a = e.length) {
		return this.tracksSourceLocation ? (this.isSourceLocationSource(t) && (i = t.start, a = t.end), {
			sourceLocationType: "inlinedSource",
			newSource: e,
			start: n,
			end: r,
			loc: t,
			startOnNew: i,
			endOnNew: a
		}) : this.gen();
	}
	isSourceLocationInlinedSource(e) {
		return this.isSourceLocation(e) && e.sourceLocationType === "inlinedSource";
	}
	gen() {
		return { sourceLocationType: "autoGenerate" };
	}
	isSourceLocationSource(e) {
		return this.isSourceLocation(e) && e.sourceLocationType === "source";
	}
	sourceLocationStringReplace(e, t, n) {
		return this.tracksSourceLocation ? {
			sourceLocationType: "stringReplace",
			newSource: e,
			start: t,
			end: n
		} : this.gen();
	}
	isSourceLocationStringReplace(e) {
		return this.isSourceLocation(e) && e.sourceLocationType === "stringReplace";
	}
	sourceLocationNodeReplaceUnsafe(e) {
		if (this.isSourceLocationSource(e)) return this.sourceLocationNodeReplace(e);
		if (this.isSourceLocationInlinedSource(e)) return this.sourceLocationNodeReplaceUnsafe(e.loc);
		throw Error(`Cannot convert SourceLocation of type ${e.sourceLocationType} to SourceLocationNodeReplace`);
	}
	sourceLocationNodeReplace(e, t) {
		let n, r;
		return typeof e == "number" ? (n = e, r = t) : (n = e.start, r = e.end), {
			sourceLocationType: "nodeReplace",
			start: n,
			end: r
		};
	}
	isSourceLocationNodeReplace(e) {
		return this.isSourceLocation(e) && e.sourceLocationType === "nodeReplace";
	}
	isSourceLocationNodeAutoGenerate(e) {
		return this.isSourceLocation(e) && e.sourceLocationType === "autoGenerate";
	}
	isPrintingLoc(e) {
		return this.isSourceLocationNodeReplace(e) || this.isSourceLocationNodeAutoGenerate(e) || this.isSourceLocationInlinedSource(e) && this.isPrintingLoc(e.loc);
	}
	printFilter(e, t) {
		this.isPrintingLoc(e.loc) && t();
	}
	isSourceLocationNoMaterialize(e) {
		return this.isSourceLocation(e) && e.sourceLocationType === "noMaterialize";
	}
	isOfType(e, t) {
		return e.type === t;
	}
	isOfSubType(e, t, n) {
		let r = e;
		return r.type === t && r.subType === n;
	}
}, t = typeof global == "object" && global && global.Object === Object && global, n = typeof self == "object" && self && self.Object === Object && self, r = t || n || Function("return this")(), i = r.Symbol, a = Object.prototype, o = a.hasOwnProperty, s = a.toString, c = i ? i.toStringTag : void 0;
function l(e) {
	var t = o.call(e, c), n = e[c];
	try {
		e[c] = void 0;
		var r = !0;
	} catch {}
	var i = s.call(e);
	return r && (t ? e[c] = n : delete e[c]), i;
}
var u = l, d = Object.prototype.toString;
function f(e) {
	return d.call(e);
}
var p = f, m = "[object Null]", h = "[object Undefined]", g = i ? i.toStringTag : void 0;
function _(e) {
	return e == null ? e === void 0 ? h : m : g && g in Object(e) ? u(e) : p(e);
}
var v = _;
function y(e) {
	return typeof e == "object" && !!e;
}
var b = y, ee = "[object Symbol]";
function x(e) {
	return typeof e == "symbol" || b(e) && v(e) == ee;
}
var te = x;
function ne(e, t) {
	for (var n = -1, r = e == null ? 0 : e.length, i = Array(r); ++n < r;) i[n] = t(e[n], n, e);
	return i;
}
var S = ne, C = Array.isArray, re = 1 / 0, ie = i ? i.prototype : void 0, ae = ie ? ie.toString : void 0;
function oe(e) {
	if (typeof e == "string") return e;
	if (C(e)) return S(e, oe) + "";
	if (te(e)) return ae ? ae.call(e) : "";
	var t = e + "";
	return t == "0" && 1 / e == -re ? "-0" : t;
}
var se = oe, ce = /\s/;
function le(e) {
	for (var t = e.length; t-- && ce.test(e.charAt(t)););
	return t;
}
var ue = le, de = /^\s+/;
function fe(e) {
	return e && e.slice(0, ue(e) + 1).replace(de, "");
}
var pe = fe;
function me(e) {
	var t = typeof e;
	return e != null && (t == "object" || t == "function");
}
var he = me, ge = NaN, _e = /^[-+]0x[0-9a-f]+$/i, ve = /^0b[01]+$/i, ye = /^0o[0-7]+$/i, be = parseInt;
function xe(e) {
	if (typeof e == "number") return e;
	if (te(e)) return ge;
	if (he(e)) {
		var t = typeof e.valueOf == "function" ? e.valueOf() : e;
		e = he(t) ? t + "" : t;
	}
	if (typeof e != "string") return e === 0 ? e : +e;
	e = pe(e);
	var n = ve.test(e);
	return n || ye.test(e) ? be(e.slice(2), n ? 2 : 8) : _e.test(e) ? ge : +e;
}
var Se = xe, Ce = 1 / 0, we = 17976931348623157e292;
function Te(e) {
	return e ? (e = Se(e), e === Ce || e === -Ce ? (e < 0 ? -1 : 1) * we : e === e ? e : 0) : e === 0 ? e : 0;
}
var Ee = Te;
function De(e) {
	var t = Ee(e), n = t % 1;
	return t === t ? n ? t - n : t : 0;
}
var Oe = De;
function ke(e) {
	return e;
}
var Ae = ke, je = "[object AsyncFunction]", Me = "[object Function]", Ne = "[object GeneratorFunction]", Pe = "[object Proxy]";
function Fe(e) {
	if (!he(e)) return !1;
	var t = v(e);
	return t == Me || t == Ne || t == je || t == Pe;
}
var Ie = Fe, Le = r["__core-js_shared__"], Re = (function() {
	var e = /[^.]+$/.exec(Le && Le.keys && Le.keys.IE_PROTO || "");
	return e ? "Symbol(src)_1." + e : "";
})();
function ze(e) {
	return !!Re && Re in e;
}
var Be = ze, Ve = Function.prototype.toString;
function He(e) {
	if (e != null) {
		try {
			return Ve.call(e);
		} catch {}
		try {
			return e + "";
		} catch {}
	}
	return "";
}
var Ue = He, We = /[\\^$.*+?()[\]{}|]/g, Ge = /^\[object .+?Constructor\]$/, Ke = Function.prototype, qe = Object.prototype, Je = Ke.toString, Ye = qe.hasOwnProperty, Xe = RegExp("^" + Je.call(Ye).replace(We, "\\$&").replace(/hasOwnProperty|(function).*?(?=\\\()| for .+?(?=\\\])/g, "$1.*?") + "$");
function Ze(e) {
	return !he(e) || Be(e) ? !1 : (Ie(e) ? Xe : Ge).test(Ue(e));
}
var Qe = Ze;
function $e(e, t) {
	return e?.[t];
}
var et = $e;
function tt(e, t) {
	var n = et(e, t);
	return Qe(n) ? n : void 0;
}
var nt = tt, rt = nt(r, "WeakMap"), it = Object.create, at = /* @__PURE__ */ (function() {
	function e() {}
	return function(t) {
		if (!he(t)) return {};
		if (it) return it(t);
		e.prototype = t;
		var n = new e();
		return e.prototype = void 0, n;
	};
})();
function ot(e, t, n) {
	switch (n.length) {
		case 0: return e.call(t);
		case 1: return e.call(t, n[0]);
		case 2: return e.call(t, n[0], n[1]);
		case 3: return e.call(t, n[0], n[1], n[2]);
	}
	return e.apply(t, n);
}
var st = ot;
function ct() {}
var w = ct;
function lt(e, t) {
	var n = -1, r = e.length;
	for (t ||= Array(r); ++n < r;) t[n] = e[n];
	return t;
}
var ut = lt, dt = 800, ft = 16, pt = Date.now;
function mt(e) {
	var t = 0, n = 0;
	return function() {
		var r = pt(), i = ft - (r - n);
		if (n = r, i > 0) {
			if (++t >= dt) return arguments[0];
		} else t = 0;
		return e.apply(void 0, arguments);
	};
}
var ht = mt;
function gt(e) {
	return function() {
		return e;
	};
}
var _t = gt, vt = (function() {
	try {
		var e = nt(Object, "defineProperty");
		return e({}, "", {}), e;
	} catch {}
})(), yt = ht(vt ? function(e, t) {
	return vt(e, "toString", {
		configurable: !0,
		enumerable: !1,
		value: _t(t),
		writable: !0
	});
} : Ae);
function bt(e, t) {
	for (var n = -1, r = e == null ? 0 : e.length; ++n < r && t(e[n], n, e) !== !1;);
	return e;
}
var xt = bt;
function St(e, t, n, r) {
	for (var i = e.length, a = n + (r ? 1 : -1); r ? a-- : ++a < i;) if (t(e[a], a, e)) return a;
	return -1;
}
var Ct = St;
function wt(e) {
	return e !== e;
}
var Tt = wt;
function Et(e, t, n) {
	for (var r = n - 1, i = e.length; ++r < i;) if (e[r] === t) return r;
	return -1;
}
var Dt = Et;
function Ot(e, t, n) {
	return t === t ? Dt(e, t, n) : Ct(e, Tt, n);
}
var kt = Ot;
function At(e, t) {
	return !!(e != null && e.length) && kt(e, t, 0) > -1;
}
var jt = At, Mt = 9007199254740991, Nt = /^(?:0|[1-9]\d*)$/;
function Pt(e, t) {
	var n = typeof e;
	return t ??= Mt, !!t && (n == "number" || n != "symbol" && Nt.test(e)) && e > -1 && e % 1 == 0 && e < t;
}
var Ft = Pt;
function It(e, t, n) {
	t == "__proto__" && vt ? vt(e, t, {
		configurable: !0,
		enumerable: !0,
		value: n,
		writable: !0
	}) : e[t] = n;
}
var Lt = It;
function Rt(e, t) {
	return e === t || e !== e && t !== t;
}
var zt = Rt, Bt = Object.prototype.hasOwnProperty;
function Vt(e, t, n) {
	var r = e[t];
	(!(Bt.call(e, t) && zt(r, n)) || n === void 0 && !(t in e)) && Lt(e, t, n);
}
var Ht = Vt;
function Ut(e, t, n, r) {
	var i = !n;
	n ||= {};
	for (var a = -1, o = t.length; ++a < o;) {
		var s = t[a], c = r ? r(n[s], e[s], s, n, e) : void 0;
		c === void 0 && (c = e[s]), i ? Lt(n, s, c) : Ht(n, s, c);
	}
	return n;
}
var Wt = Ut, Gt = Math.max;
function Kt(e, t, n) {
	return t = Gt(t === void 0 ? e.length - 1 : t, 0), function() {
		for (var r = arguments, i = -1, a = Gt(r.length - t, 0), o = Array(a); ++i < a;) o[i] = r[t + i];
		i = -1;
		for (var s = Array(t + 1); ++i < t;) s[i] = r[i];
		return s[t] = n(o), st(e, this, s);
	};
}
var qt = Kt;
function Jt(e, t) {
	return yt(qt(e, t, Ae), e + "");
}
var Yt = Jt, Xt = 9007199254740991;
function Zt(e) {
	return typeof e == "number" && e > -1 && e % 1 == 0 && e <= Xt;
}
var Qt = Zt;
function $t(e) {
	return e != null && Qt(e.length) && !Ie(e);
}
var en = $t;
function tn(e, t, n) {
	if (!he(n)) return !1;
	var r = typeof t;
	return (r == "number" ? en(n) && Ft(t, n.length) : r == "string" && t in n) ? zt(n[t], e) : !1;
}
var nn = tn;
function rn(e) {
	return Yt(function(t, n) {
		var r = -1, i = n.length, a = i > 1 ? n[i - 1] : void 0, o = i > 2 ? n[2] : void 0;
		for (a = e.length > 3 && typeof a == "function" ? (i--, a) : void 0, o && nn(n[0], n[1], o) && (a = i < 3 ? void 0 : a, i = 1), t = Object(t); ++r < i;) {
			var s = n[r];
			s && e(t, s, r, a);
		}
		return t;
	});
}
var an = rn, on = Object.prototype;
function sn(e) {
	var t = e && e.constructor;
	return e === (typeof t == "function" && t.prototype || on);
}
var cn = sn;
function ln(e, t) {
	for (var n = -1, r = Array(e); ++n < e;) r[n] = t(n);
	return r;
}
var un = ln, dn = "[object Arguments]";
function fn(e) {
	return b(e) && v(e) == dn;
}
var pn = fn, mn = Object.prototype, hn = mn.hasOwnProperty, gn = mn.propertyIsEnumerable, _n = pn(/* @__PURE__ */ (function() {
	return arguments;
})()) ? pn : function(e) {
	return b(e) && hn.call(e, "callee") && !gn.call(e, "callee");
};
function vn() {
	return !1;
}
var yn = vn, bn = typeof exports == "object" && exports && !exports.nodeType && exports, xn = bn && typeof module == "object" && module && !module.nodeType && module, Sn = xn && xn.exports === bn ? r.Buffer : void 0, Cn = (Sn ? Sn.isBuffer : void 0) || yn, wn = "[object Arguments]", Tn = "[object Array]", En = "[object Boolean]", Dn = "[object Date]", On = "[object Error]", kn = "[object Function]", An = "[object Map]", jn = "[object Number]", Mn = "[object Object]", Nn = "[object RegExp]", Pn = "[object Set]", Fn = "[object String]", In = "[object WeakMap]", Ln = "[object ArrayBuffer]", Rn = "[object DataView]", zn = "[object Float32Array]", Bn = "[object Float64Array]", Vn = "[object Int8Array]", Hn = "[object Int16Array]", Un = "[object Int32Array]", Wn = "[object Uint8Array]", Gn = "[object Uint8ClampedArray]", Kn = "[object Uint16Array]", qn = "[object Uint32Array]", T = {};
T[zn] = T[Bn] = T[Vn] = T[Hn] = T[Un] = T[Wn] = T[Gn] = T[Kn] = T[qn] = !0, T[wn] = T[Tn] = T[Ln] = T[En] = T[Rn] = T[Dn] = T[On] = T[kn] = T[An] = T[jn] = T[Mn] = T[Nn] = T[Pn] = T[Fn] = T[In] = !1;
function Jn(e) {
	return b(e) && Qt(e.length) && !!T[v(e)];
}
var Yn = Jn;
function Xn(e) {
	return function(t) {
		return e(t);
	};
}
var Zn = Xn, Qn = typeof exports == "object" && exports && !exports.nodeType && exports, $n = Qn && typeof module == "object" && module && !module.nodeType && module, er = $n && $n.exports === Qn && t.process, tr = (function() {
	try {
		return $n && $n.require && $n.require("util").types || er && er.binding && er.binding("util");
	} catch {}
})(), nr = tr && tr.isTypedArray, rr = nr ? Zn(nr) : Yn, ir = Object.prototype.hasOwnProperty;
function ar(e, t) {
	var n = C(e), r = !n && _n(e), i = !n && !r && Cn(e), a = !n && !r && !i && rr(e), o = n || r || i || a, s = o ? un(e.length, String) : [], c = s.length;
	for (var l in e) (t || ir.call(e, l)) && !(o && (l == "length" || i && (l == "offset" || l == "parent") || a && (l == "buffer" || l == "byteLength" || l == "byteOffset") || Ft(l, c))) && s.push(l);
	return s;
}
var or = ar;
function sr(e, t) {
	return function(n) {
		return e(t(n));
	};
}
var cr = sr, lr = cr(Object.keys, Object), ur = Object.prototype.hasOwnProperty;
function dr(e) {
	if (!cn(e)) return lr(e);
	var t = [];
	for (var n in Object(e)) ur.call(e, n) && n != "constructor" && t.push(n);
	return t;
}
var fr = dr;
function pr(e) {
	return en(e) ? or(e) : fr(e);
}
var mr = pr, hr = Object.prototype.hasOwnProperty, gr = an(function(e, t) {
	if (cn(t) || en(t)) {
		Wt(t, mr(t), e);
		return;
	}
	for (var n in t) hr.call(t, n) && Ht(e, n, t[n]);
});
function _r(e) {
	var t = [];
	if (e != null) for (var n in Object(e)) t.push(n);
	return t;
}
var vr = _r, yr = Object.prototype.hasOwnProperty;
function br(e) {
	if (!he(e)) return vr(e);
	var t = cn(e), n = [];
	for (var r in e) (r != "constructor" || !t && yr.call(e, r)) && n.push(r);
	return n;
}
var xr = br;
function Sr(e) {
	return en(e) ? or(e, !0) : xr(e);
}
var Cr = Sr, wr = /\.|\[(?:[^[\]]*|(["'])(?:(?!\1)[^\\]|\\.)*?\1)\]/, Tr = /^\w*$/;
function Er(e, t) {
	if (C(e)) return !1;
	var n = typeof e;
	return n == "number" || n == "symbol" || n == "boolean" || e == null || te(e) ? !0 : Tr.test(e) || !wr.test(e) || t != null && e in Object(t);
}
var Dr = Er, Or = nt(Object, "create");
function kr() {
	this.__data__ = Or ? Or(null) : {}, this.size = 0;
}
var Ar = kr;
function jr(e) {
	var t = this.has(e) && delete this.__data__[e];
	return this.size -= +!!t, t;
}
var Mr = jr, Nr = "__lodash_hash_undefined__", Pr = Object.prototype.hasOwnProperty;
function Fr(e) {
	var t = this.__data__;
	if (Or) {
		var n = t[e];
		return n === Nr ? void 0 : n;
	}
	return Pr.call(t, e) ? t[e] : void 0;
}
var Ir = Fr, Lr = Object.prototype.hasOwnProperty;
function Rr(e) {
	var t = this.__data__;
	return Or ? t[e] !== void 0 : Lr.call(t, e);
}
var zr = Rr, Br = "__lodash_hash_undefined__";
function Vr(e, t) {
	var n = this.__data__;
	return this.size += +!this.has(e), n[e] = Or && t === void 0 ? Br : t, this;
}
var Hr = Vr;
function Ur(e) {
	var t = -1, n = e == null ? 0 : e.length;
	for (this.clear(); ++t < n;) {
		var r = e[t];
		this.set(r[0], r[1]);
	}
}
Ur.prototype.clear = Ar, Ur.prototype.delete = Mr, Ur.prototype.get = Ir, Ur.prototype.has = zr, Ur.prototype.set = Hr;
var Wr = Ur;
function Gr() {
	this.__data__ = [], this.size = 0;
}
var Kr = Gr;
function qr(e, t) {
	for (var n = e.length; n--;) if (zt(e[n][0], t)) return n;
	return -1;
}
var Jr = qr, Yr = Array.prototype.splice;
function Xr(e) {
	var t = this.__data__, n = Jr(t, e);
	return n < 0 ? !1 : (n == t.length - 1 ? t.pop() : Yr.call(t, n, 1), --this.size, !0);
}
var Zr = Xr;
function Qr(e) {
	var t = this.__data__, n = Jr(t, e);
	return n < 0 ? void 0 : t[n][1];
}
var $r = Qr;
function ei(e) {
	return Jr(this.__data__, e) > -1;
}
var ti = ei;
function ni(e, t) {
	var n = this.__data__, r = Jr(n, e);
	return r < 0 ? (++this.size, n.push([e, t])) : n[r][1] = t, this;
}
var ri = ni;
function ii(e) {
	var t = -1, n = e == null ? 0 : e.length;
	for (this.clear(); ++t < n;) {
		var r = e[t];
		this.set(r[0], r[1]);
	}
}
ii.prototype.clear = Kr, ii.prototype.delete = Zr, ii.prototype.get = $r, ii.prototype.has = ti, ii.prototype.set = ri;
var ai = ii, oi = nt(r, "Map");
function si() {
	this.size = 0, this.__data__ = {
		hash: new Wr(),
		map: new (oi || ai)(),
		string: new Wr()
	};
}
var ci = si;
function li(e) {
	var t = typeof e;
	return t == "string" || t == "number" || t == "symbol" || t == "boolean" ? e !== "__proto__" : e === null;
}
var ui = li;
function di(e, t) {
	var n = e.__data__;
	return ui(t) ? n[typeof t == "string" ? "string" : "hash"] : n.map;
}
var fi = di;
function pi(e) {
	var t = fi(this, e).delete(e);
	return this.size -= +!!t, t;
}
var mi = pi;
function hi(e) {
	return fi(this, e).get(e);
}
var gi = hi;
function _i(e) {
	return fi(this, e).has(e);
}
var vi = _i;
function yi(e, t) {
	var n = fi(this, e), r = n.size;
	return n.set(e, t), this.size += n.size == r ? 0 : 1, this;
}
var bi = yi;
function xi(e) {
	var t = -1, n = e == null ? 0 : e.length;
	for (this.clear(); ++t < n;) {
		var r = e[t];
		this.set(r[0], r[1]);
	}
}
xi.prototype.clear = ci, xi.prototype.delete = mi, xi.prototype.get = gi, xi.prototype.has = vi, xi.prototype.set = bi;
var Si = xi, Ci = "Expected a function";
function wi(e, t) {
	if (typeof e != "function" || t != null && typeof t != "function") throw TypeError(Ci);
	var n = function() {
		var r = arguments, i = t ? t.apply(this, r) : r[0], a = n.cache;
		if (a.has(i)) return a.get(i);
		var o = e.apply(this, r);
		return n.cache = a.set(i, o) || a, o;
	};
	return n.cache = new (wi.Cache || Si)(), n;
}
wi.Cache = Si;
var Ti = wi, Ei = 500;
function Di(e) {
	var t = Ti(e, function(e) {
		return n.size === Ei && n.clear(), e;
	}), n = t.cache;
	return t;
}
var Oi = Di, ki = /[^.[\]]+|\[(?:(-?\d+(?:\.\d+)?)|(["'])((?:(?!\2)[^\\]|\\.)*?)\2)\]|(?=(?:\.|\[\])(?:\.|\[\]|$))/g, Ai = /\\(\\)?/g, ji = Oi(function(e) {
	var t = [];
	return e.charCodeAt(0) === 46 && t.push(""), e.replace(ki, function(e, n, r, i) {
		t.push(r ? i.replace(Ai, "$1") : n || e);
	}), t;
});
function Mi(e) {
	return e == null ? "" : se(e);
}
var Ni = Mi;
function Pi(e, t) {
	return C(e) ? e : Dr(e, t) ? [e] : ji(Ni(e));
}
var Fi = Pi, Ii = 1 / 0;
function Li(e) {
	if (typeof e == "string" || te(e)) return e;
	var t = e + "";
	return t == "0" && 1 / e == -Ii ? "-0" : t;
}
var Ri = Li;
function zi(e, t) {
	t = Fi(t, e);
	for (var n = 0, r = t.length; e != null && n < r;) e = e[Ri(t[n++])];
	return n && n == r ? e : void 0;
}
var Bi = zi;
function Vi(e, t, n) {
	var r = e == null ? void 0 : Bi(e, t);
	return r === void 0 ? n : r;
}
var Hi = Vi;
function Ui(e, t) {
	for (var n = -1, r = t.length, i = e.length; ++n < r;) e[i + n] = t[n];
	return e;
}
var Wi = Ui, Gi = i ? i.isConcatSpreadable : void 0;
function Ki(e) {
	return C(e) || _n(e) || !!(Gi && e && e[Gi]);
}
var qi = Ki;
function Ji(e, t, n, r, i) {
	var a = -1, o = e.length;
	for (n ||= qi, i ||= []; ++a < o;) {
		var s = e[a];
		t > 0 && n(s) ? t > 1 ? Ji(s, t - 1, n, r, i) : Wi(i, s) : r || (i[i.length] = s);
	}
	return i;
}
var Yi = Ji;
function Xi(e) {
	return e != null && e.length ? Yi(e, 1) : [];
}
var Zi = Xi, Qi = cr(Object.getPrototypeOf, Object);
function $i(e, t, n) {
	var r = -1, i = e.length;
	t < 0 && (t = -t > i ? 0 : i + t), n = n > i ? i : n, n < 0 && (n += i), i = t > n ? 0 : n - t >>> 0, t >>>= 0;
	for (var a = Array(i); ++r < i;) a[r] = e[r + t];
	return a;
}
var ea = $i, ta = "\\ud800-\\udfff", na = "\\u0300-\\u036f\\ufe20-\\ufe2f\\u20d0-\\u20ff", ra = "\\ufe0e\\ufe0f", ia = "[" + ta + "]", aa = "[" + na + "]", oa = "\\ud83c[\\udffb-\\udfff]", sa = "(?:" + aa + "|" + oa + ")", ca = "[^" + ta + "]", la = "(?:\\ud83c[\\udde6-\\uddff]){2}", ua = "[\\ud800-\\udbff][\\udc00-\\udfff]", da = "\\u200d", fa = sa + "?", pa = "[" + ra + "]?", ma = "(?:" + da + "(?:" + [
	ca,
	la,
	ua
].join("|") + ")" + pa + fa + ")*", ha = pa + fa + ma, ga = "(?:" + [
	ca + aa + "?",
	aa,
	la,
	ua,
	ia
].join("|") + ")";
RegExp(oa + "(?=" + oa + ")|" + ga + ha, "g");
function _a(e, t, n, r) {
	var i = -1, a = e == null ? 0 : e.length;
	for (r && a && (n = e[++i]); ++i < a;) n = t(n, e[i], i, e);
	return n;
}
var va = _a;
function ya() {
	this.__data__ = new ai(), this.size = 0;
}
var ba = ya;
function xa(e) {
	var t = this.__data__, n = t.delete(e);
	return this.size = t.size, n;
}
var Sa = xa;
function Ca(e) {
	return this.__data__.get(e);
}
var wa = Ca;
function Ta(e) {
	return this.__data__.has(e);
}
var Ea = Ta, Da = 200;
function Oa(e, t) {
	var n = this.__data__;
	if (n instanceof ai) {
		var r = n.__data__;
		if (!oi || r.length < Da - 1) return r.push([e, t]), this.size = ++n.size, this;
		n = this.__data__ = new Si(r);
	}
	return n.set(e, t), this.size = n.size, this;
}
var ka = Oa;
function Aa(e) {
	var t = this.__data__ = new ai(e);
	this.size = t.size;
}
Aa.prototype.clear = ba, Aa.prototype.delete = Sa, Aa.prototype.get = wa, Aa.prototype.has = Ea, Aa.prototype.set = ka;
var ja = Aa;
function Ma(e, t) {
	return e && Wt(t, mr(t), e);
}
var Na = Ma;
function Pa(e, t) {
	return e && Wt(t, Cr(t), e);
}
var Fa = Pa, Ia = typeof exports == "object" && exports && !exports.nodeType && exports, La = Ia && typeof module == "object" && module && !module.nodeType && module, Ra = La && La.exports === Ia ? r.Buffer : void 0, za = Ra ? Ra.allocUnsafe : void 0;
function Ba(e, t) {
	if (t) return e.slice();
	var n = e.length, r = za ? za(n) : new e.constructor(n);
	return e.copy(r), r;
}
var Va = Ba;
function Ha(e, t) {
	for (var n = -1, r = e == null ? 0 : e.length, i = 0, a = []; ++n < r;) {
		var o = e[n];
		t(o, n, e) && (a[i++] = o);
	}
	return a;
}
var Ua = Ha;
function Wa() {
	return [];
}
var Ga = Wa, Ka = Object.prototype.propertyIsEnumerable, qa = Object.getOwnPropertySymbols, Ja = qa ? function(e) {
	return e == null ? [] : (e = Object(e), Ua(qa(e), function(t) {
		return Ka.call(e, t);
	}));
} : Ga;
function Ya(e, t) {
	return Wt(e, Ja(e), t);
}
var Xa = Ya, Za = Object.getOwnPropertySymbols ? function(e) {
	for (var t = []; e;) Wi(t, Ja(e)), e = Qi(e);
	return t;
} : Ga;
function Qa(e, t) {
	return Wt(e, Za(e), t);
}
var $a = Qa;
function eo(e, t, n) {
	var r = t(e);
	return C(e) ? r : Wi(r, n(e));
}
var to = eo;
function no(e) {
	return to(e, mr, Ja);
}
var ro = no;
function io(e) {
	return to(e, Cr, Za);
}
var ao = io, oo = nt(r, "DataView"), so = nt(r, "Promise"), co = nt(r, "Set"), lo = "[object Map]", uo = "[object Object]", fo = "[object Promise]", po = "[object Set]", mo = "[object WeakMap]", ho = "[object DataView]", go = Ue(oo), _o = Ue(oi), vo = Ue(so), yo = Ue(co), bo = Ue(rt), xo = v;
(oo && xo(new oo(/* @__PURE__ */ new ArrayBuffer(1))) != ho || oi && xo(new oi()) != lo || so && xo(so.resolve()) != fo || co && xo(new co()) != po || rt && xo(new rt()) != mo) && (xo = function(e) {
	var t = v(e), n = t == uo ? e.constructor : void 0, r = n ? Ue(n) : "";
	if (r) switch (r) {
		case go: return ho;
		case _o: return lo;
		case vo: return fo;
		case yo: return po;
		case bo: return mo;
	}
	return t;
});
var So = xo, Co = Object.prototype.hasOwnProperty;
function wo(e) {
	var t = e.length, n = new e.constructor(t);
	return t && typeof e[0] == "string" && Co.call(e, "index") && (n.index = e.index, n.input = e.input), n;
}
var To = wo, Eo = r.Uint8Array;
function Do(e) {
	var t = new e.constructor(e.byteLength);
	return new Eo(t).set(new Eo(e)), t;
}
var Oo = Do;
function ko(e, t) {
	var n = t ? Oo(e.buffer) : e.buffer;
	return new e.constructor(n, e.byteOffset, e.byteLength);
}
var Ao = ko, jo = /\w*$/;
function Mo(e) {
	var t = new e.constructor(e.source, jo.exec(e));
	return t.lastIndex = e.lastIndex, t;
}
var No = Mo, Po = i ? i.prototype : void 0, Fo = Po ? Po.valueOf : void 0;
function Io(e) {
	return Fo ? Object(Fo.call(e)) : {};
}
var Lo = Io;
function Ro(e, t) {
	var n = t ? Oo(e.buffer) : e.buffer;
	return new e.constructor(n, e.byteOffset, e.length);
}
var zo = Ro, Bo = "[object Boolean]", Vo = "[object Date]", Ho = "[object Map]", Uo = "[object Number]", Wo = "[object RegExp]", Go = "[object Set]", Ko = "[object String]", qo = "[object Symbol]", Jo = "[object ArrayBuffer]", Yo = "[object DataView]", Xo = "[object Float32Array]", Zo = "[object Float64Array]", Qo = "[object Int8Array]", $o = "[object Int16Array]", es = "[object Int32Array]", ts = "[object Uint8Array]", ns = "[object Uint8ClampedArray]", rs = "[object Uint16Array]", is = "[object Uint32Array]";
function as(e, t, n) {
	var r = e.constructor;
	switch (t) {
		case Jo: return Oo(e);
		case Bo:
		case Vo: return new r(+e);
		case Yo: return Ao(e, n);
		case Xo:
		case Zo:
		case Qo:
		case $o:
		case es:
		case ts:
		case ns:
		case rs:
		case is: return zo(e, n);
		case Ho: return new r();
		case Uo:
		case Ko: return new r(e);
		case Wo: return No(e);
		case Go: return new r();
		case qo: return Lo(e);
	}
}
var os = as;
function ss(e) {
	return typeof e.constructor == "function" && !cn(e) ? at(Qi(e)) : {};
}
var cs = ss, ls = "[object Map]";
function us(e) {
	return b(e) && So(e) == ls;
}
var ds = us, fs = tr && tr.isMap, ps = fs ? Zn(fs) : ds, ms = "[object Set]";
function hs(e) {
	return b(e) && So(e) == ms;
}
var gs = hs, _s = tr && tr.isSet, vs = _s ? Zn(_s) : gs, ys = 1, bs = 2, xs = 4, Ss = "[object Arguments]", Cs = "[object Array]", ws = "[object Boolean]", Ts = "[object Date]", Es = "[object Error]", Ds = "[object Function]", Os = "[object GeneratorFunction]", ks = "[object Map]", As = "[object Number]", js = "[object Object]", Ms = "[object RegExp]", Ns = "[object Set]", Ps = "[object String]", Fs = "[object Symbol]", Is = "[object WeakMap]", Ls = "[object ArrayBuffer]", Rs = "[object DataView]", zs = "[object Float32Array]", Bs = "[object Float64Array]", Vs = "[object Int8Array]", Hs = "[object Int16Array]", Us = "[object Int32Array]", Ws = "[object Uint8Array]", Gs = "[object Uint8ClampedArray]", Ks = "[object Uint16Array]", qs = "[object Uint32Array]", E = {};
E[Ss] = E[Cs] = E[Ls] = E[Rs] = E[ws] = E[Ts] = E[zs] = E[Bs] = E[Vs] = E[Hs] = E[Us] = E[ks] = E[As] = E[js] = E[Ms] = E[Ns] = E[Ps] = E[Fs] = E[Ws] = E[Gs] = E[Ks] = E[qs] = !0, E[Es] = E[Ds] = E[Is] = !1;
function Js(e, t, n, r, i, a) {
	var o, s = t & ys, c = t & bs, l = t & xs;
	if (n && (o = i ? n(e, r, i, a) : n(e)), o !== void 0) return o;
	if (!he(e)) return e;
	var u = C(e);
	if (u) {
		if (o = To(e), !s) return ut(e, o);
	} else {
		var d = So(e), f = d == Ds || d == Os;
		if (Cn(e)) return Va(e, s);
		if (d == js || d == Ss || f && !i) {
			if (o = c || f ? {} : cs(e), !s) return c ? $a(e, Fa(o, e)) : Xa(e, Na(o, e));
		} else {
			if (!E[d]) return i ? e : {};
			o = os(e, d, s);
		}
	}
	a ||= new ja();
	var p = a.get(e);
	if (p) return p;
	a.set(e, o), vs(e) ? e.forEach(function(r) {
		o.add(Js(r, t, n, r, e, a));
	}) : ps(e) && e.forEach(function(r, i) {
		o.set(i, Js(r, t, n, i, e, a));
	});
	var m = u ? void 0 : (l ? c ? ao : ro : c ? Cr : mr)(e);
	return xt(m || e, function(r, i) {
		m && (i = r, r = e[i]), Ht(o, i, Js(r, t, n, i, e, a));
	}), o;
}
var Ys = Js, Xs = 4;
function Zs(e) {
	return Ys(e, Xs);
}
var Qs = Zs;
function $s(e) {
	for (var t = -1, n = e == null ? 0 : e.length, r = 0, i = []; ++t < n;) {
		var a = e[t];
		a && (i[r++] = a);
	}
	return i;
}
var ec = $s, tc = "__lodash_hash_undefined__";
function nc(e) {
	return this.__data__.set(e, tc), this;
}
var rc = nc;
function ic(e) {
	return this.__data__.has(e);
}
var ac = ic;
function oc(e) {
	var t = -1, n = e == null ? 0 : e.length;
	for (this.__data__ = new Si(); ++t < n;) this.add(e[t]);
}
oc.prototype.add = oc.prototype.push = rc, oc.prototype.has = ac;
var sc = oc;
function cc(e, t) {
	for (var n = -1, r = e == null ? 0 : e.length; ++n < r;) if (t(e[n], n, e)) return !0;
	return !1;
}
var lc = cc;
function uc(e, t) {
	return e.has(t);
}
var dc = uc, fc = 1, pc = 2;
function mc(e, t, n, r, i, a) {
	var o = n & fc, s = e.length, c = t.length;
	if (s != c && !(o && c > s)) return !1;
	var l = a.get(e), u = a.get(t);
	if (l && u) return l == t && u == e;
	var d = -1, f = !0, p = n & pc ? new sc() : void 0;
	for (a.set(e, t), a.set(t, e); ++d < s;) {
		var m = e[d], h = t[d];
		if (r) var g = o ? r(h, m, d, t, e, a) : r(m, h, d, e, t, a);
		if (g !== void 0) {
			if (g) continue;
			f = !1;
			break;
		}
		if (p) {
			if (!lc(t, function(e, t) {
				if (!dc(p, t) && (m === e || i(m, e, n, r, a))) return p.push(t);
			})) {
				f = !1;
				break;
			}
		} else if (!(m === h || i(m, h, n, r, a))) {
			f = !1;
			break;
		}
	}
	return a.delete(e), a.delete(t), f;
}
var hc = mc;
function gc(e) {
	var t = -1, n = Array(e.size);
	return e.forEach(function(e, r) {
		n[++t] = [r, e];
	}), n;
}
var _c = gc;
function vc(e) {
	var t = -1, n = Array(e.size);
	return e.forEach(function(e) {
		n[++t] = e;
	}), n;
}
var yc = vc, bc = 1, xc = 2, Sc = "[object Boolean]", Cc = "[object Date]", wc = "[object Error]", Tc = "[object Map]", Ec = "[object Number]", Dc = "[object RegExp]", Oc = "[object Set]", kc = "[object String]", Ac = "[object Symbol]", jc = "[object ArrayBuffer]", Mc = "[object DataView]", Nc = i ? i.prototype : void 0, Pc = Nc ? Nc.valueOf : void 0;
function Fc(e, t, n, r, i, a, o) {
	switch (n) {
		case Mc:
			if (e.byteLength != t.byteLength || e.byteOffset != t.byteOffset) return !1;
			e = e.buffer, t = t.buffer;
		case jc: return !(e.byteLength != t.byteLength || !a(new Eo(e), new Eo(t)));
		case Sc:
		case Cc:
		case Ec: return zt(+e, +t);
		case wc: return e.name == t.name && e.message == t.message;
		case Dc:
		case kc: return e == t + "";
		case Tc: var s = _c;
		case Oc:
			var c = r & bc;
			if (s ||= yc, e.size != t.size && !c) return !1;
			var l = o.get(e);
			if (l) return l == t;
			r |= xc, o.set(e, t);
			var u = hc(s(e), s(t), r, i, a, o);
			return o.delete(e), u;
		case Ac: if (Pc) return Pc.call(e) == Pc.call(t);
	}
	return !1;
}
var Ic = Fc, Lc = 1, Rc = Object.prototype.hasOwnProperty;
function zc(e, t, n, r, i, a) {
	var o = n & Lc, s = ro(e), c = s.length;
	if (c != ro(t).length && !o) return !1;
	for (var l = c; l--;) {
		var u = s[l];
		if (!(o ? u in t : Rc.call(t, u))) return !1;
	}
	var d = a.get(e), f = a.get(t);
	if (d && f) return d == t && f == e;
	var p = !0;
	a.set(e, t), a.set(t, e);
	for (var m = o; ++l < c;) {
		u = s[l];
		var h = e[u], g = t[u];
		if (r) var _ = o ? r(g, h, u, t, e, a) : r(h, g, u, e, t, a);
		if (!(_ === void 0 ? h === g || i(h, g, n, r, a) : _)) {
			p = !1;
			break;
		}
		m ||= u == "constructor";
	}
	if (p && !m) {
		var v = e.constructor, y = t.constructor;
		v != y && "constructor" in e && "constructor" in t && !(typeof v == "function" && v instanceof v && typeof y == "function" && y instanceof y) && (p = !1);
	}
	return a.delete(e), a.delete(t), p;
}
var Bc = zc, Vc = 1, Hc = "[object Arguments]", Uc = "[object Array]", Wc = "[object Object]", Gc = Object.prototype.hasOwnProperty;
function Kc(e, t, n, r, i, a) {
	var o = C(e), s = C(t), c = o ? Uc : So(e), l = s ? Uc : So(t);
	c = c == Hc ? Wc : c, l = l == Hc ? Wc : l;
	var u = c == Wc, d = l == Wc, f = c == l;
	if (f && Cn(e)) {
		if (!Cn(t)) return !1;
		o = !0, u = !1;
	}
	if (f && !u) return a ||= new ja(), o || rr(e) ? hc(e, t, n, r, i, a) : Ic(e, t, c, n, r, i, a);
	if (!(n & Vc)) {
		var p = u && Gc.call(e, "__wrapped__"), m = d && Gc.call(t, "__wrapped__");
		if (p || m) {
			var h = p ? e.value() : e, g = m ? t.value() : t;
			return a ||= new ja(), i(h, g, n, r, a);
		}
	}
	return f ? (a ||= new ja(), Bc(e, t, n, r, i, a)) : !1;
}
var qc = Kc;
function Jc(e, t, n, r, i) {
	return e === t ? !0 : e == null || t == null || !b(e) && !b(t) ? e !== e && t !== t : qc(e, t, n, r, Jc, i);
}
var Yc = Jc, Xc = 1, Zc = 2;
function Qc(e, t, n, r) {
	var i = n.length, a = i, o = !r;
	if (e == null) return !a;
	for (e = Object(e); i--;) {
		var s = n[i];
		if (o && s[2] ? s[1] !== e[s[0]] : !(s[0] in e)) return !1;
	}
	for (; ++i < a;) {
		s = n[i];
		var c = s[0], l = e[c], u = s[1];
		if (o && s[2]) {
			if (l === void 0 && !(c in e)) return !1;
		} else {
			var d = new ja();
			if (r) var f = r(l, u, c, e, t, d);
			if (!(f === void 0 ? Yc(u, l, Xc | Zc, r, d) : f)) return !1;
		}
	}
	return !0;
}
var $c = Qc;
function el(e) {
	return e === e && !he(e);
}
var tl = el;
function nl(e) {
	for (var t = mr(e), n = t.length; n--;) {
		var r = t[n], i = e[r];
		t[n] = [
			r,
			i,
			tl(i)
		];
	}
	return t;
}
var rl = nl;
function il(e, t) {
	return function(n) {
		return n != null && n[e] === t && (t !== void 0 || e in Object(n));
	};
}
var al = il;
function ol(e) {
	var t = rl(e);
	return t.length == 1 && t[0][2] ? al(t[0][0], t[0][1]) : function(n) {
		return n === e || $c(n, e, t);
	};
}
var sl = ol;
function cl(e, t) {
	return e != null && t in Object(e);
}
var ll = cl;
function ul(e, t, n) {
	t = Fi(t, e);
	for (var r = -1, i = t.length, a = !1; ++r < i;) {
		var o = Ri(t[r]);
		if (!(a = e != null && n(e, o))) break;
		e = e[o];
	}
	return a || ++r != i ? a : (i = e == null ? 0 : e.length, !!i && Qt(i) && Ft(o, i) && (C(e) || _n(e)));
}
var dl = ul;
function fl(e, t) {
	return e != null && dl(e, t, ll);
}
var pl = fl, ml = 1, hl = 2;
function gl(e, t) {
	return Dr(e) && tl(t) ? al(Ri(e), t) : function(n) {
		var r = Hi(n, e);
		return r === void 0 && r === t ? pl(n, e) : Yc(t, r, ml | hl);
	};
}
var _l = gl;
function vl(e) {
	return function(t) {
		return t?.[e];
	};
}
var yl = vl;
function bl(e) {
	return function(t) {
		return Bi(t, e);
	};
}
var xl = bl;
function Sl(e) {
	return Dr(e) ? yl(Ri(e)) : xl(e);
}
var Cl = Sl;
function wl(e) {
	return typeof e == "function" ? e : e == null ? Ae : typeof e == "object" ? C(e) ? _l(e[0], e[1]) : sl(e) : Cl(e);
}
var Tl = wl;
function El(e, t, n, r) {
	for (var i = -1, a = e == null ? 0 : e.length; ++i < a;) {
		var o = e[i];
		t(r, o, n(o), e);
	}
	return r;
}
var Dl = El;
function Ol(e) {
	return function(t, n, r) {
		for (var i = -1, a = Object(t), o = r(t), s = o.length; s--;) {
			var c = o[e ? s : ++i];
			if (n(a[c], c, a) === !1) break;
		}
		return t;
	};
}
var kl = Ol();
function Al(e, t) {
	return e && kl(e, t, mr);
}
var jl = Al;
function Ml(e, t) {
	return function(n, r) {
		if (n == null) return n;
		if (!en(n)) return e(n, r);
		for (var i = n.length, a = t ? i : -1, o = Object(n); (t ? a-- : ++a < i) && r(o[a], a, o) !== !1;);
		return n;
	};
}
var Nl = Ml(jl);
function Pl(e, t, n, r) {
	return Nl(e, function(e, i, a) {
		t(r, e, n(e), a);
	}), r;
}
var Fl = Pl;
function Il(e, t) {
	return function(n, r) {
		var i = C(n) ? Dl : Fl, a = t ? t() : {};
		return i(n, e, Tl(r, 2), a);
	};
}
var Ll = Il, Rl = Object.prototype, zl = Rl.hasOwnProperty, Bl = Yt(function(e, t) {
	e = Object(e);
	var n = -1, r = t.length, i = r > 2 ? t[2] : void 0;
	for (i && nn(t[0], t[1], i) && (r = 1); ++n < r;) for (var a = t[n], o = Cr(a), s = -1, c = o.length; ++s < c;) {
		var l = o[s], u = e[l];
		(u === void 0 || zt(u, Rl[l]) && !zl.call(e, l)) && (e[l] = a[l]);
	}
	return e;
});
function Vl(e) {
	return b(e) && en(e);
}
var Hl = Vl;
function Ul(e, t, n) {
	for (var r = -1, i = e == null ? 0 : e.length; ++r < i;) if (n(t, e[r])) return !0;
	return !1;
}
var Wl = Ul, Gl = 200;
function Kl(e, t, n, r) {
	var i = -1, a = jt, o = !0, s = e.length, c = [], l = t.length;
	if (!s) return c;
	n && (t = S(t, Zn(n))), r ? (a = Wl, o = !1) : t.length >= Gl && (a = dc, o = !1, t = new sc(t));
	outer: for (; ++i < s;) {
		var u = e[i], d = n == null ? u : n(u);
		if (u = r || u !== 0 ? u : 0, o && d === d) {
			for (var f = l; f--;) if (t[f] === d) continue outer;
			c.push(u);
		} else a(t, d, r) || c.push(u);
	}
	return c;
}
var ql = Kl, Jl = Yt(function(e, t) {
	return Hl(e) ? ql(e, Yi(t, 1, Hl, !0)) : [];
});
function Yl(e) {
	var t = e == null ? 0 : e.length;
	return t ? e[t - 1] : void 0;
}
var Xl = Yl;
function Zl(e, t, n) {
	var r = e == null ? 0 : e.length;
	return r ? (t = n || t === void 0 ? 1 : Oe(t), ea(e, t < 0 ? 0 : t, r)) : [];
}
var D = Zl;
function Ql(e, t, n) {
	var r = e == null ? 0 : e.length;
	return r ? (t = n || t === void 0 ? 1 : Oe(t), t = r - t, ea(e, 0, t < 0 ? 0 : t)) : [];
}
var $l = Ql;
function eu(e) {
	return typeof e == "function" ? e : Ae;
}
var tu = eu;
function nu(e, t) {
	return (C(e) ? xt : Nl)(e, tu(t));
}
var O = nu;
function ru(e, t) {
	for (var n = -1, r = e == null ? 0 : e.length; ++n < r;) if (!t(e[n], n, e)) return !1;
	return !0;
}
var iu = ru;
function au(e, t) {
	var n = !0;
	return Nl(e, function(e, r, i) {
		return n = !!t(e, r, i), n;
	}), n;
}
var ou = au;
function su(e, t, n) {
	var r = C(e) ? iu : ou;
	return n && nn(e, t, n) && (t = void 0), r(e, Tl(t, 3));
}
var cu = su;
function lu(e, t) {
	var n = [];
	return Nl(e, function(e, r, i) {
		t(e, r, i) && n.push(e);
	}), n;
}
var uu = lu;
function du(e, t) {
	return (C(e) ? Ua : uu)(e, Tl(t, 3));
}
var fu = du;
function pu(e) {
	return function(t, n, r) {
		var i = Object(t);
		if (!en(t)) {
			var a = Tl(n, 3);
			t = mr(t), n = function(e) {
				return a(i[e], e, i);
			};
		}
		var o = e(t, n, r);
		return o > -1 ? i[a ? t[o] : o] : void 0;
	};
}
var mu = pu, hu = Math.max;
function gu(e, t, n) {
	var r = e == null ? 0 : e.length;
	if (!r) return -1;
	var i = n == null ? 0 : Oe(n);
	return i < 0 && (i = hu(r + i, 0)), Ct(e, Tl(t, 3), i);
}
var _u = mu(gu);
function vu(e) {
	return e && e.length ? e[0] : void 0;
}
var yu = vu;
function bu(e, t) {
	var n = -1, r = en(e) ? Array(e.length) : [];
	return Nl(e, function(e, i, a) {
		r[++n] = t(e, i, a);
	}), r;
}
var xu = bu;
function Su(e, t) {
	return (C(e) ? S : xu)(e, Tl(t, 3));
}
var k = Su;
function Cu(e, t) {
	return Yi(k(e, t), 1);
}
var wu = Cu, Tu = Object.prototype.hasOwnProperty, Eu = Ll(function(e, t, n) {
	Tu.call(e, n) ? e[n].push(t) : Lt(e, n, [t]);
}), Du = Object.prototype.hasOwnProperty;
function Ou(e, t) {
	return e != null && Du.call(e, t);
}
var ku = Ou;
function Au(e, t) {
	return e != null && dl(e, t, ku);
}
var A = Au, ju = "[object String]";
function Mu(e) {
	return typeof e == "string" || !C(e) && b(e) && v(e) == ju;
}
var Nu = Mu;
function Pu(e, t) {
	return S(t, function(t) {
		return e[t];
	});
}
var Fu = Pu;
function Iu(e) {
	return e == null ? [] : Fu(e, mr(e));
}
var j = Iu, Lu = Math.max;
function Ru(e, t, n, r) {
	e = en(e) ? e : j(e), n = n && !r ? Oe(n) : 0;
	var i = e.length;
	return n < 0 && (n = Lu(i + n, 0)), Nu(e) ? n <= i && e.indexOf(t, n) > -1 : !!i && kt(e, t, n) > -1;
}
var zu = Ru, Bu = Math.max;
function Vu(e, t, n) {
	var r = e == null ? 0 : e.length;
	if (!r) return -1;
	var i = n == null ? 0 : Oe(n);
	return i < 0 && (i = Bu(r + i, 0)), kt(e, t, i);
}
var Hu = Vu, Uu = "[object Map]", Wu = "[object Set]", Gu = Object.prototype.hasOwnProperty;
function Ku(e) {
	if (e == null) return !0;
	if (en(e) && (C(e) || typeof e == "string" || typeof e.splice == "function" || Cn(e) || rr(e) || _n(e))) return !e.length;
	var t = So(e);
	if (t == Uu || t == Wu) return !e.size;
	if (cn(e)) return !fr(e).length;
	for (var n in e) if (Gu.call(e, n)) return !1;
	return !0;
}
var M = Ku, qu = "[object RegExp]";
function Ju(e) {
	return b(e) && v(e) == qu;
}
var Yu = Ju, Xu = tr && tr.isRegExp, Zu = Xu ? Zn(Xu) : Yu;
function Qu(e) {
	return e === void 0;
}
var $u = Qu, ed = "Expected a function";
function td(e) {
	if (typeof e != "function") throw TypeError(ed);
	return function() {
		var t = arguments;
		switch (t.length) {
			case 0: return !e.call(this);
			case 1: return !e.call(this, t[0]);
			case 2: return !e.call(this, t[0], t[1]);
			case 3: return !e.call(this, t[0], t[1], t[2]);
		}
		return !e.apply(this, t);
	};
}
var nd = td;
function rd(e, t, n, r) {
	if (!he(e)) return e;
	t = Fi(t, e);
	for (var i = -1, a = t.length, o = a - 1, s = e; s != null && ++i < a;) {
		var c = Ri(t[i]), l = n;
		if (c === "__proto__" || c === "constructor" || c === "prototype") return e;
		if (i != o) {
			var u = s[c];
			l = r ? r(u, c, s) : void 0, l === void 0 && (l = he(u) ? u : Ft(t[i + 1]) ? [] : {});
		}
		Ht(s, c, l), s = s[c];
	}
	return e;
}
var id = rd;
function ad(e, t, n) {
	for (var r = -1, i = t.length, a = {}; ++r < i;) {
		var o = t[r], s = Bi(e, o);
		n(s, o) && id(a, Fi(o, e), s);
	}
	return a;
}
var od = ad;
function sd(e, t) {
	if (e == null) return {};
	var n = S(ao(e), function(e) {
		return [e];
	});
	return t = Tl(t), od(e, n, function(e, n) {
		return t(e, n[0]);
	});
}
var cd = sd;
function ld(e, t, n, r, i) {
	return i(e, function(e, i, a) {
		n = r ? (r = !1, e) : t(n, e, i, a);
	}), n;
}
var ud = ld;
function dd(e, t, n) {
	var r = C(e) ? va : ud, i = arguments.length < 3;
	return r(e, Tl(t, 4), n, i, Nl);
}
var fd = dd;
function pd(e, t) {
	return (C(e) ? Ua : uu)(e, nd(Tl(t, 3)));
}
var md = pd;
function hd(e, t) {
	var n;
	return Nl(e, function(e, r, i) {
		return n = t(e, r, i), !n;
	}), !!n;
}
var gd = hd;
function _d(e, t, n) {
	var r = C(e) ? lc : gd;
	return n && nn(e, t, n) && (t = void 0), r(e, Tl(t, 3));
}
var vd = _d, yd = co && 1 / yc(new co([, -0]))[1] == 1 / 0 ? function(e) {
	return new co(e);
} : w, bd = 200;
function xd(e, t, n) {
	var r = -1, i = jt, a = e.length, o = !0, s = [], c = s;
	if (n) o = !1, i = Wl;
	else if (a >= bd) {
		var l = t ? null : yd(e);
		if (l) return yc(l);
		o = !1, i = dc, c = new sc();
	} else c = t ? [] : s;
	outer: for (; ++r < a;) {
		var u = e[r], d = t ? t(u) : u;
		if (u = n || u !== 0 ? u : 0, o && d === d) {
			for (var f = c.length; f--;) if (c[f] === d) continue outer;
			t && c.push(d), s.push(u);
		} else i(c, d, n) || (c !== s && c.push(d), s.push(u));
	}
	return s;
}
var Sd = xd;
function Cd(e) {
	return e && e.length ? Sd(e) : [];
}
var wd = Cd;
function Td(e) {
	console && console.error && console.error(`Error: ${e}`);
}
function Ed(e) {
	console && console.warn && console.warn(`Warning: ${e}`);
}
function Dd(e) {
	let t = (/* @__PURE__ */ new Date()).getTime(), n = e();
	return {
		time: (/* @__PURE__ */ new Date()).getTime() - t,
		value: n
	};
}
function Od(e) {
	function t() {}
	t.prototype = e;
	let n = new t();
	function r() {
		return typeof n.bar;
	}
	return r(), r(), e;
}
function kd(e) {
	return Ad(e) ? e.LABEL : e.name;
}
function Ad(e) {
	return Nu(e.LABEL) && e.LABEL !== "";
}
var jd = class {
	get definition() {
		return this._definition;
	}
	set definition(e) {
		this._definition = e;
	}
	constructor(e) {
		this._definition = e;
	}
	accept(e) {
		e.visit(this), O(this.definition, (t) => {
			t.accept(e);
		});
	}
}, Md = class extends jd {
	constructor(e) {
		super([]), this.idx = 1, gr(this, cd(e, (e) => e !== void 0));
	}
	set definition(e) {}
	get definition() {
		return this.referencedRule === void 0 ? [] : this.referencedRule.definition;
	}
	accept(e) {
		e.visit(this);
	}
}, Nd = class extends jd {
	constructor(e) {
		super(e.definition), this.orgText = "", gr(this, cd(e, (e) => e !== void 0));
	}
}, Pd = class extends jd {
	constructor(e) {
		super(e.definition), this.ignoreAmbiguities = !1, gr(this, cd(e, (e) => e !== void 0));
	}
}, Fd = class extends jd {
	constructor(e) {
		super(e.definition), this.idx = 1, gr(this, cd(e, (e) => e !== void 0));
	}
}, Id = class extends jd {
	constructor(e) {
		super(e.definition), this.idx = 1, gr(this, cd(e, (e) => e !== void 0));
	}
}, Ld = class extends jd {
	constructor(e) {
		super(e.definition), this.idx = 1, gr(this, cd(e, (e) => e !== void 0));
	}
}, N = class extends jd {
	constructor(e) {
		super(e.definition), this.idx = 1, gr(this, cd(e, (e) => e !== void 0));
	}
}, Rd = class extends jd {
	constructor(e) {
		super(e.definition), this.idx = 1, gr(this, cd(e, (e) => e !== void 0));
	}
}, zd = class extends jd {
	get definition() {
		return this._definition;
	}
	set definition(e) {
		this._definition = e;
	}
	constructor(e) {
		super(e.definition), this.idx = 1, this.ignoreAmbiguities = !1, this.hasPredicates = !1, gr(this, cd(e, (e) => e !== void 0));
	}
}, P = class {
	constructor(e) {
		this.idx = 1, gr(this, cd(e, (e) => e !== void 0));
	}
	accept(e) {
		e.visit(this);
	}
};
function Bd(e) {
	return k(e, Vd);
}
function Vd(e) {
	function t(e) {
		return k(e, Vd);
	}
	if (e instanceof Md) {
		let t = {
			type: "NonTerminal",
			name: e.nonTerminalName,
			idx: e.idx
		};
		return Nu(e.label) && (t.label = e.label), t;
	}
	if (e instanceof Pd) return {
		type: "Alternative",
		definition: t(e.definition)
	};
	if (e instanceof Fd) return {
		type: "Option",
		idx: e.idx,
		definition: t(e.definition)
	};
	if (e instanceof Id) return {
		type: "RepetitionMandatory",
		idx: e.idx,
		definition: t(e.definition)
	};
	if (e instanceof Ld) return {
		type: "RepetitionMandatoryWithSeparator",
		idx: e.idx,
		separator: Vd(new P({ terminalType: e.separator })),
		definition: t(e.definition)
	};
	if (e instanceof Rd) return {
		type: "RepetitionWithSeparator",
		idx: e.idx,
		separator: Vd(new P({ terminalType: e.separator })),
		definition: t(e.definition)
	};
	if (e instanceof N) return {
		type: "Repetition",
		idx: e.idx,
		definition: t(e.definition)
	};
	if (e instanceof zd) return {
		type: "Alternation",
		idx: e.idx,
		definition: t(e.definition)
	};
	if (e instanceof P) {
		let t = {
			type: "Terminal",
			name: e.terminalType.name,
			label: kd(e.terminalType),
			idx: e.idx
		};
		Nu(e.label) && (t.terminalLabel = e.label);
		let n = e.terminalType.PATTERN;
		return e.terminalType.PATTERN && (t.pattern = Zu(n) ? n.source : n), t;
	}
	if (e instanceof Nd) return {
		type: "Rule",
		name: e.name,
		orgText: e.orgText,
		definition: t(e.definition)
	};
	throw Error("non exhaustive match");
}
var Hd = class {
	visit(e) {
		let t = e;
		switch (t.constructor) {
			case Md: return this.visitNonTerminal(t);
			case Pd: return this.visitAlternative(t);
			case Fd: return this.visitOption(t);
			case Id: return this.visitRepetitionMandatory(t);
			case Ld: return this.visitRepetitionMandatoryWithSeparator(t);
			case Rd: return this.visitRepetitionWithSeparator(t);
			case N: return this.visitRepetition(t);
			case zd: return this.visitAlternation(t);
			case P: return this.visitTerminal(t);
			case Nd: return this.visitRule(t);
			/* c8 ignore next 2 */
			default: throw Error("non exhaustive match");
		}
	}
	/* c8 ignore next */
	visitNonTerminal(e) {}
	/* c8 ignore next */
	visitAlternative(e) {}
	/* c8 ignore next */
	visitOption(e) {}
	/* c8 ignore next */
	visitRepetition(e) {}
	/* c8 ignore next */
	visitRepetitionMandatory(e) {}
	/* c8 ignore next 3 */
	visitRepetitionMandatoryWithSeparator(e) {}
	/* c8 ignore next */
	visitRepetitionWithSeparator(e) {}
	/* c8 ignore next */
	visitAlternation(e) {}
	/* c8 ignore next */
	visitTerminal(e) {}
	/* c8 ignore next */
	visitRule(e) {}
};
function Ud(e) {
	return e instanceof Pd || e instanceof Fd || e instanceof N || e instanceof Id || e instanceof Ld || e instanceof Rd || e instanceof P || e instanceof Nd;
}
function Wd(e, t = []) {
	return e instanceof Fd || e instanceof N || e instanceof Rd ? !0 : e instanceof zd ? vd(e.definition, (e) => Wd(e, t)) : e instanceof Md && zu(t, e) ? !1 : e instanceof jd && (e instanceof Md && t.push(e), cu(e.definition, (e) => Wd(e, t)));
}
function Gd(e) {
	return e instanceof zd;
}
function Kd(e) {
	if (e instanceof Md) return "SUBRULE";
	if (e instanceof Fd) return "OPTION";
	if (e instanceof zd) return "OR";
	if (e instanceof Id) return "AT_LEAST_ONE";
	if (e instanceof Ld) return "AT_LEAST_ONE_SEP";
	if (e instanceof Rd) return "MANY_SEP";
	if (e instanceof N) return "MANY";
	if (e instanceof P) return "CONSUME";
	throw Error("non exhaustive match");
}
var qd = class {
	walk(e, t = []) {
		O(e.definition, (n, r) => {
			let i = D(e.definition, r + 1);
			if (n instanceof Md) this.walkProdRef(n, i, t);
			else if (n instanceof P) this.walkTerminal(n, i, t);
			else if (n instanceof Pd) this.walkFlat(n, i, t);
			else if (n instanceof Fd) this.walkOption(n, i, t);
			else if (n instanceof Id) this.walkAtLeastOne(n, i, t);
			else if (n instanceof Ld) this.walkAtLeastOneSep(n, i, t);
			else if (n instanceof Rd) this.walkManySep(n, i, t);
			else if (n instanceof N) this.walkMany(n, i, t);
			else if (n instanceof zd) this.walkOr(n, i, t);
			else throw Error("non exhaustive match");
		});
	}
	walkTerminal(e, t, n) {}
	walkProdRef(e, t, n) {}
	walkFlat(e, t, n) {
		let r = t.concat(n);
		this.walk(e, r);
	}
	walkOption(e, t, n) {
		let r = t.concat(n);
		this.walk(e, r);
	}
	walkAtLeastOne(e, t, n) {
		let r = [new Fd({ definition: e.definition })].concat(t, n);
		this.walk(e, r);
	}
	walkAtLeastOneSep(e, t, n) {
		let r = Jd(e, t, n);
		this.walk(e, r);
	}
	walkMany(e, t, n) {
		let r = [new Fd({ definition: e.definition })].concat(t, n);
		this.walk(e, r);
	}
	walkManySep(e, t, n) {
		let r = Jd(e, t, n);
		this.walk(e, r);
	}
	walkOr(e, t, n) {
		let r = t.concat(n);
		O(e.definition, (e) => {
			let t = new Pd({ definition: [e] });
			this.walk(t, r);
		});
	}
};
function Jd(e, t, n) {
	return [new Fd({ definition: [new P({ terminalType: e.separator })].concat(e.definition) })].concat(t, n);
}
function Yd(e) {
	if (e instanceof Md) return Yd(e.referencedRule);
	if (e instanceof P) return Qd(e);
	if (Ud(e)) return Xd(e);
	if (Gd(e)) return Zd(e);
	throw Error("non exhaustive match");
}
function Xd(e) {
	let t = [], n = e.definition, r = 0, i = n.length > r, a, o = !0;
	for (; i && o;) a = n[r], o = Wd(a), t = t.concat(Yd(a)), r += 1, i = n.length > r;
	return wd(t);
}
function Zd(e) {
	return wd(Zi(k(e.definition, (e) => Yd(e))));
}
function Qd(e) {
	return [e.terminalType];
}
var $d = "_~IN~_", ef = class extends qd {
	constructor(e) {
		super(), this.topProd = e, this.follows = {};
	}
	startWalking() {
		return this.walk(this.topProd), this.follows;
	}
	walkTerminal(e, t, n) {}
	walkProdRef(e, t, n) {
		let r = nf(e.referencedRule, e.idx) + this.topProd.name, i = Yd(new Pd({ definition: t.concat(n) }));
		this.follows[r] = i;
	}
};
function tf(e) {
	let t = {};
	return O(e, (e) => {
		let n = new ef(e).startWalking();
		gr(t, n);
	}), t;
}
function nf(e, t) {
	return e.name + t + $d;
}
function F(e) {
	return e.charCodeAt(0);
}
function rf(e, t) {
	Array.isArray(e) ? e.forEach(function(e) {
		t.push(e);
	}) : t.push(e);
}
function af(e, t) {
	if (e[t] === !0) throw "duplicate flag " + t;
	e[t], e[t] = !0;
}
function of(e) {
	if (e === void 0) throw Error("Internal Error - Should never get here!");
	return !0;
}
function sf() {
	throw Error("Internal Error - Should never get here!");
}
function cf(e) {
	return e.type === "Character";
}
var lf = [];
for (let e = F("0"); e <= F("9"); e++) lf.push(e);
var uf = [F("_")].concat(lf);
for (let e = F("a"); e <= F("z"); e++) uf.push(e);
for (let e = F("A"); e <= F("Z"); e++) uf.push(e);
var df = [
	F(" "),
	F("\f"),
	F("\n"),
	F("\r"),
	F("	"),
	F("\v"),
	F("	"),
	F("\xA0"),
	F(" "),
	F(" "),
	F(" "),
	F(" "),
	F(" "),
	F(" "),
	F(" "),
	F(" "),
	F(" "),
	F(" "),
	F(" "),
	F(" "),
	F("\u2028"),
	F("\u2029"),
	F(" "),
	F(" "),
	F("　"),
	F("﻿")
], ff = /[0-9a-fA-F]/, pf = /[0-9]/, mf = /[1-9]/, hf = class {
	constructor() {
		this.idx = 0, this.input = "", this.groupIdx = 0;
	}
	saveState() {
		return {
			idx: this.idx,
			input: this.input,
			groupIdx: this.groupIdx
		};
	}
	restoreState(e) {
		this.idx = e.idx, this.input = e.input, this.groupIdx = e.groupIdx;
	}
	pattern(e) {
		this.idx = 0, this.input = e, this.groupIdx = 0, this.consumeChar("/");
		let t = this.disjunction();
		this.consumeChar("/");
		let n = {
			type: "Flags",
			loc: {
				begin: this.idx,
				end: e.length
			},
			global: !1,
			ignoreCase: !1,
			multiLine: !1,
			unicode: !1,
			sticky: !1
		};
		for (; this.isRegExpFlag();) switch (this.popChar()) {
			case "g":
				af(n, "global");
				break;
			case "i":
				af(n, "ignoreCase");
				break;
			case "m":
				af(n, "multiLine");
				break;
			case "u":
				af(n, "unicode");
				break;
			case "y": af(n, "sticky");
		}
		if (this.idx !== this.input.length) throw Error("Redundant input: " + this.input.substring(this.idx));
		return {
			type: "Pattern",
			flags: n,
			value: t,
			loc: this.loc(0)
		};
	}
	disjunction() {
		let e = [], t = this.idx;
		for (e.push(this.alternative()); this.peekChar() === "|";) this.consumeChar("|"), e.push(this.alternative());
		return {
			type: "Disjunction",
			value: e,
			loc: this.loc(t)
		};
	}
	alternative() {
		let e = [], t = this.idx;
		for (; this.isTerm();) e.push(this.term());
		return {
			type: "Alternative",
			value: e,
			loc: this.loc(t)
		};
	}
	term() {
		return this.isAssertion() ? this.assertion() : this.atom();
	}
	assertion() {
		let e = this.idx;
		switch (this.popChar()) {
			case "^": return {
				type: "StartAnchor",
				loc: this.loc(e)
			};
			case "$": return {
				type: "EndAnchor",
				loc: this.loc(e)
			};
			case "\\":
				switch (this.popChar()) {
					case "b": return {
						type: "WordBoundary",
						loc: this.loc(e)
					};
					case "B": return {
						type: "NonWordBoundary",
						loc: this.loc(e)
					};
				}
				throw Error("Invalid Assertion Escape");
			case "(":
				this.consumeChar("?");
				let t;
				switch (this.popChar()) {
					case "=":
						t = "Lookahead";
						break;
					case "!":
						t = "NegativeLookahead";
						break;
					case "<": switch (this.popChar()) {
						case "=":
							t = "Lookbehind";
							break;
						case "!": t = "NegativeLookbehind";
					}
				}
				of(t);
				let n = this.disjunction();
				return this.consumeChar(")"), {
					type: t,
					value: n,
					loc: this.loc(e)
				};
		}
		return sf();
	}
	quantifier(e = !1) {
		let t, n = this.idx;
		switch (this.popChar()) {
			case "*":
				t = {
					atLeast: 0,
					atMost: Infinity
				};
				break;
			case "+":
				t = {
					atLeast: 1,
					atMost: Infinity
				};
				break;
			case "?":
				t = {
					atLeast: 0,
					atMost: 1
				};
				break;
			case "{":
				let n = this.integerIncludingZero();
				switch (this.popChar()) {
					case "}":
						t = {
							atLeast: n,
							atMost: n
						};
						break;
					case ",":
						let e;
						this.isDigit() ? (e = this.integerIncludingZero(), t = {
							atLeast: n,
							atMost: e
						}) : t = {
							atLeast: n,
							atMost: Infinity
						}, this.consumeChar("}");
				}
				if (e === !0 && t === void 0) return;
				of(t);
		}
		if ((e !== !0 || t !== void 0) && of(t)) return this.peekChar(0) === "?" ? (this.consumeChar("?"), t.greedy = !1) : t.greedy = !0, t.type = "Quantifier", t.loc = this.loc(n), t;
	}
	atom() {
		let e, t = this.idx;
		switch (this.peekChar()) {
			case ".":
				e = this.dotAll();
				break;
			case "\\":
				e = this.atomEscape();
				break;
			case "[":
				e = this.characterClass();
				break;
			case "(": e = this.group();
		}
		return e === void 0 && this.isPatternCharacter() && (e = this.patternCharacter()), of(e) ? (e.loc = this.loc(t), this.isQuantifier() && (e.quantifier = this.quantifier()), e) : sf();
	}
	dotAll() {
		return this.consumeChar("."), {
			type: "Set",
			complement: !0,
			value: [
				F("\n"),
				F("\r"),
				F("\u2028"),
				F("\u2029")
			]
		};
	}
	atomEscape() {
		switch (this.consumeChar("\\"), this.peekChar()) {
			case "1":
			case "2":
			case "3":
			case "4":
			case "5":
			case "6":
			case "7":
			case "8":
			case "9": return this.decimalEscapeAtom();
			case "d":
			case "D":
			case "s":
			case "S":
			case "w":
			case "W": return this.characterClassEscape();
			case "f":
			case "n":
			case "r":
			case "t":
			case "v": return this.controlEscapeAtom();
			case "c": return this.controlLetterEscapeAtom();
			case "0": return this.nulCharacterAtom();
			case "x": return this.hexEscapeSequenceAtom();
			case "u": return this.regExpUnicodeEscapeSequenceAtom();
			default: return this.identityEscapeAtom();
		}
	}
	decimalEscapeAtom() {
		return {
			type: "GroupBackReference",
			value: this.positiveInteger()
		};
	}
	characterClassEscape() {
		let e, t = !1;
		switch (this.popChar()) {
			case "d":
				e = lf;
				break;
			case "D":
				e = lf, t = !0;
				break;
			case "s":
				e = df;
				break;
			case "S":
				e = df, t = !0;
				break;
			case "w":
				e = uf;
				break;
			case "W": e = uf, t = !0;
		}
		return of(e) ? {
			type: "Set",
			value: e,
			complement: t
		} : sf();
	}
	controlEscapeAtom() {
		let e;
		switch (this.popChar()) {
			case "f":
				e = F("\f");
				break;
			case "n":
				e = F("\n");
				break;
			case "r":
				e = F("\r");
				break;
			case "t":
				e = F("	");
				break;
			case "v": e = F("\v");
		}
		return of(e) ? {
			type: "Character",
			value: e
		} : sf();
	}
	controlLetterEscapeAtom() {
		this.consumeChar("c");
		let e = this.popChar();
		if (/[a-zA-Z]/.test(e) === !1) throw Error("Invalid ");
		return {
			type: "Character",
			value: e.toUpperCase().charCodeAt(0) - 64
		};
	}
	nulCharacterAtom() {
		return this.consumeChar("0"), {
			type: "Character",
			value: F("\0")
		};
	}
	hexEscapeSequenceAtom() {
		return this.consumeChar("x"), this.parseHexDigits(2);
	}
	regExpUnicodeEscapeSequenceAtom() {
		return this.consumeChar("u"), this.parseHexDigits(4);
	}
	identityEscapeAtom() {
		return {
			type: "Character",
			value: F(this.popChar())
		};
	}
	classPatternCharacterAtom() {
		switch (this.peekChar()) {
			// istanbul ignore next
			case "\n":
			// istanbul ignore next
			case "\r":
			// istanbul ignore next
			case "\u2028":
			// istanbul ignore next
			case "\u2029":
			// istanbul ignore next
			case "\\":
			// istanbul ignore next
			case "]": throw Error("TBD");
			default: return {
				type: "Character",
				value: F(this.popChar())
			};
		}
	}
	characterClass() {
		let e = [], t = !1;
		for (this.consumeChar("["), this.peekChar(0) === "^" && (this.consumeChar("^"), t = !0); this.isClassAtom();) {
			let t = this.classAtom();
			if (t.type, cf(t) && this.isRangeDash()) {
				this.consumeChar("-");
				let n = this.classAtom();
				if (n.type, cf(n)) {
					if (n.value < t.value) throw Error("Range out of order in character class");
					e.push({
						from: t.value,
						to: n.value
					});
				} else rf(t.value, e), e.push(F("-")), rf(n.value, e);
			} else rf(t.value, e);
		}
		return this.consumeChar("]"), {
			type: "Set",
			complement: t,
			value: e
		};
	}
	classAtom() {
		switch (this.peekChar()) {
			// istanbul ignore next
			case "]":
			// istanbul ignore next
			case "\n":
			// istanbul ignore next
			case "\r":
			// istanbul ignore next
			case "\u2028":
			// istanbul ignore next
			case "\u2029": throw Error("TBD");
			case "\\": return this.classEscape();
			default: return this.classPatternCharacterAtom();
		}
	}
	classEscape() {
		switch (this.consumeChar("\\"), this.peekChar()) {
			case "b": return this.consumeChar("b"), {
				type: "Character",
				value: F("\b")
			};
			case "d":
			case "D":
			case "s":
			case "S":
			case "w":
			case "W": return this.characterClassEscape();
			case "f":
			case "n":
			case "r":
			case "t":
			case "v": return this.controlEscapeAtom();
			case "c": return this.controlLetterEscapeAtom();
			case "0": return this.nulCharacterAtom();
			case "x": return this.hexEscapeSequenceAtom();
			case "u": return this.regExpUnicodeEscapeSequenceAtom();
			default: return this.identityEscapeAtom();
		}
	}
	group() {
		let e = !0;
		switch (this.consumeChar("("), this.peekChar(0)) {
			case "?":
				this.consumeChar("?"), this.consumeChar(":"), e = !1;
				break;
			default: this.groupIdx++;
		}
		let t = this.disjunction();
		this.consumeChar(")");
		let n = {
			type: "Group",
			capturing: e,
			value: t
		};
		return e && (n.idx = this.groupIdx), n;
	}
	positiveInteger() {
		let e = this.popChar();
		if (mf.test(e) === !1) throw Error("Expecting a positive integer");
		for (; pf.test(this.peekChar(0));) e += this.popChar();
		return parseInt(e, 10);
	}
	integerIncludingZero() {
		let e = this.popChar();
		if (pf.test(e) === !1) throw Error("Expecting an integer");
		for (; pf.test(this.peekChar(0));) e += this.popChar();
		return parseInt(e, 10);
	}
	patternCharacter() {
		let e = this.popChar();
		switch (e) {
			// istanbul ignore next
			case "\n":
			// istanbul ignore next
			case "\r":
			// istanbul ignore next
			case "\u2028":
			// istanbul ignore next
			case "\u2029":
			// istanbul ignore next
			case "^":
			// istanbul ignore next
			case "$":
			// istanbul ignore next
			case "\\":
			// istanbul ignore next
			case ".":
			// istanbul ignore next
			case "*":
			// istanbul ignore next
			case "+":
			// istanbul ignore next
			case "?":
			// istanbul ignore next
			case "(":
			// istanbul ignore next
			case ")":
			// istanbul ignore next
			case "[":
			// istanbul ignore next
			case "|": throw Error("TBD");
			default: return {
				type: "Character",
				value: F(e)
			};
		}
	}
	isRegExpFlag() {
		switch (this.peekChar(0)) {
			case "g":
			case "i":
			case "m":
			case "u":
			case "y": return !0;
			default: return !1;
		}
	}
	isRangeDash() {
		return this.peekChar() === "-" && this.isClassAtom(1);
	}
	isDigit() {
		return pf.test(this.peekChar(0));
	}
	isClassAtom(e = 0) {
		switch (this.peekChar(e)) {
			case "]":
			case "\n":
			case "\r":
			case "\u2028":
			case "\u2029": return !1;
			default: return !0;
		}
	}
	isTerm() {
		return this.isAtom() || this.isAssertion();
	}
	isAtom() {
		if (this.isPatternCharacter()) return !0;
		switch (this.peekChar(0)) {
			case ".":
			case "\\":
			case "[":
			case "(": return !0;
			default: return !1;
		}
	}
	isAssertion() {
		switch (this.peekChar(0)) {
			case "^":
			case "$": return !0;
			case "\\": switch (this.peekChar(1)) {
				case "b":
				case "B": return !0;
				default: return !1;
			}
			case "(": return this.peekChar(1) === "?" && (this.peekChar(2) === "=" || this.peekChar(2) === "!" || this.peekChar(2) === "<" && (this.peekChar(3) === "=" || this.peekChar(3) === "!"));
			default: return !1;
		}
	}
	isQuantifier() {
		let e = this.saveState();
		try {
			return this.quantifier(!0) !== void 0;
		} catch {
			return !1;
		} finally {
			this.restoreState(e);
		}
	}
	isPatternCharacter() {
		switch (this.peekChar()) {
			case "^":
			case "$":
			case "\\":
			case ".":
			case "*":
			case "+":
			case "?":
			case "(":
			case ")":
			case "[":
			case "|":
			case "/":
			case "\n":
			case "\r":
			case "\u2028":
			case "\u2029": return !1;
			default: return !0;
		}
	}
	parseHexDigits(e) {
		let t = "";
		for (let n = 0; n < e; n++) {
			let e = this.popChar();
			if (ff.test(e) === !1) throw Error("Expecting a HexDecimal digits");
			t += e;
		}
		return {
			type: "Character",
			value: parseInt(t, 16)
		};
	}
	peekChar(e = 0) {
		return this.input[this.idx + e];
	}
	popChar() {
		let e = this.peekChar(0);
		return this.consumeChar(void 0), e;
	}
	consumeChar(e) {
		if (e !== void 0 && this.input[this.idx] !== e) throw Error("Expected: '" + e + "' but found: '" + this.input[this.idx] + "' at offset: " + this.idx);
		if (this.idx >= this.input.length) throw Error("Unexpected end of input");
		this.idx++;
	}
	loc(e) {
		return {
			begin: e,
			end: this.idx
		};
	}
}, gf = class {
	visitChildren(e) {
		for (let t in e) {
			let n = e[t];
			e.hasOwnProperty(t) && (n.type === void 0 ? Array.isArray(n) && n.forEach((e) => {
				this.visit(e);
			}, this) : this.visit(n));
		}
	}
	visit(e) {
		switch (e.type) {
			case "Pattern":
				this.visitPattern(e);
				break;
			case "Flags":
				this.visitFlags(e);
				break;
			case "Disjunction":
				this.visitDisjunction(e);
				break;
			case "Alternative":
				this.visitAlternative(e);
				break;
			case "StartAnchor":
				this.visitStartAnchor(e);
				break;
			case "EndAnchor":
				this.visitEndAnchor(e);
				break;
			case "WordBoundary":
				this.visitWordBoundary(e);
				break;
			case "NonWordBoundary":
				this.visitNonWordBoundary(e);
				break;
			case "Lookahead":
				this.visitLookahead(e);
				break;
			case "NegativeLookahead":
				this.visitNegativeLookahead(e);
				break;
			case "Lookbehind":
				this.visitLookbehind(e);
				break;
			case "NegativeLookbehind":
				this.visitNegativeLookbehind(e);
				break;
			case "Character":
				this.visitCharacter(e);
				break;
			case "Set":
				this.visitSet(e);
				break;
			case "Group":
				this.visitGroup(e);
				break;
			case "GroupBackReference":
				this.visitGroupBackReference(e);
				break;
			case "Quantifier": this.visitQuantifier(e);
		}
		this.visitChildren(e);
	}
	visitPattern(e) {}
	visitFlags(e) {}
	visitDisjunction(e) {}
	visitAlternative(e) {}
	visitStartAnchor(e) {}
	visitEndAnchor(e) {}
	visitWordBoundary(e) {}
	visitNonWordBoundary(e) {}
	visitLookahead(e) {}
	visitNegativeLookahead(e) {}
	visitLookbehind(e) {}
	visitNegativeLookbehind(e) {}
	visitCharacter(e) {}
	visitSet(e) {}
	visitGroup(e) {}
	visitGroupBackReference(e) {}
	visitQuantifier(e) {}
}, _f = {}, vf = new hf();
function yf(e) {
	let t = e.toString();
	if (_f.hasOwnProperty(t)) return _f[t];
	{
		let e = vf.pattern(t);
		return _f[t] = e, e;
	}
}
function bf() {
	_f = {};
}
var xf = "Complement Sets are not supported for first char optimization", Sf = "Unable to use \"first char\" lexer optimizations:\n";
function Cf(e, t = !1) {
	try {
		let t = yf(e);
		return wf(t.value, {}, t.flags.ignoreCase);
	} catch (n) {
		if (n.message === xf) t && Ed(`${Sf}	Unable to optimize: < ${e.toString()} >
	Complement Sets cannot be automatically optimized.
	This will disable the lexer's first char optimizations.
	See: https://chevrotain.io/docs/guide/resolving_lexer_errors.html#COMPLEMENT for details.`);
		else {
			let n = "";
			t && (n = "\n	This will disable the lexer's first char optimizations.\n	See: https://chevrotain.io/docs/guide/resolving_lexer_errors.html#REGEXP_PARSING for details."), Td(`${Sf}
	Failed parsing: < ${e.toString()} >
	Using the @chevrotain/regexp-to-ast library
	Please open an issue at: https://github.com/chevrotain/chevrotain/issues` + n);
		}
	}
	return [];
}
function wf(e, t, n) {
	switch (e.type) {
		case "Disjunction":
			for (let r = 0; r < e.value.length; r++) wf(e.value[r], t, n);
			break;
		case "Alternative":
			let r = e.value;
			for (let e = 0; e < r.length; e++) {
				let i = r[e];
				switch (i.type) {
					case "EndAnchor":
					case "GroupBackReference":
					case "Lookahead":
					case "NegativeLookahead":
					case "Lookbehind":
					case "NegativeLookbehind":
					case "StartAnchor":
					case "WordBoundary":
					case "NonWordBoundary": continue;
				}
				let a = i;
				switch (a.type) {
					case "Character":
						Tf(a.value, t, n);
						break;
					case "Set":
						if (a.complement === !0) throw Error(xf);
						O(a.value, (e) => {
							if (typeof e == "number") Tf(e, t, n);
							else {
								let r = e;
								if (n === !0) for (let e = r.from; e <= r.to; e++) Tf(e, t, n);
								else {
									for (let e = r.from; e <= r.to && e < lp; e++) Tf(e, t, n);
									if (r.to >= lp) {
										let e = r.from >= lp ? r.from : lp, n = r.to, i = dp(e), a = dp(n);
										for (let e = i; e <= a; e++) t[e] = e;
									}
								}
							}
						});
						break;
					case "Group":
						wf(a.value, t, n);
						break;
					/* istanbul ignore next */
					default: throw Error("Non Exhaustive Match");
				}
				let o = a.quantifier !== void 0 && a.quantifier.atLeast === 0;
				if (a.type === "Group" && Of(a) === !1 || a.type !== "Group" && o === !1) break;
			}
			break;
		/* istanbul ignore next */
		default: throw Error("non exhaustive match!");
	}
	return j(t);
}
function Tf(e, t, n) {
	let r = dp(e);
	t[r] = r, n === !0 && Ef(e, t);
}
function Ef(e, t) {
	let n = String.fromCharCode(e), r = n.toUpperCase();
	if (r !== n) {
		let e = dp(r.charCodeAt(0));
		t[e] = e;
	} else {
		let e = n.toLowerCase();
		if (e !== n) {
			let n = dp(e.charCodeAt(0));
			t[n] = n;
		}
	}
}
function Df(e, t) {
	return _u(e.value, (e) => {
		if (typeof e == "number") return zu(t, e);
		{
			let n = e;
			return _u(t, (e) => n.from <= e && e <= n.to) !== void 0;
		}
	});
}
function Of(e) {
	let t = e.quantifier;
	return t && t.atLeast === 0 ? !0 : e.value ? C(e.value) ? cu(e.value, Of) : Of(e.value) : !1;
}
var kf = class extends gf {
	constructor(e) {
		super(), this.targetCharCodes = e, this.found = !1;
	}
	visitChildren(e) {
		if (this.found !== !0) {
			switch (e.type) {
				case "Lookahead":
					this.visitLookahead(e);
					return;
				case "NegativeLookahead":
					this.visitNegativeLookahead(e);
					return;
				case "Lookbehind":
					this.visitLookbehind(e);
					return;
				case "NegativeLookbehind":
					this.visitNegativeLookbehind(e);
					return;
			}
			super.visitChildren(e);
		}
	}
	visitCharacter(e) {
		zu(this.targetCharCodes, e.value) && (this.found = !0);
	}
	visitSet(e) {
		e.complement ? Df(e, this.targetCharCodes) === void 0 && (this.found = !0) : Df(e, this.targetCharCodes) !== void 0 && (this.found = !0);
	}
};
function Af(e, t) {
	if (t instanceof RegExp) {
		let n = yf(t), r = new kf(e);
		return r.visit(n), r.found;
	}
	return _u(t, (t) => zu(e, t.charCodeAt(0))) !== void 0;
}
var jf = "PATTERN", Mf = "defaultMode", Nf = "modes";
function Pf(e, t) {
	t = Bl(t, {
		debug: !1,
		safeMode: !1,
		positionTracking: "full",
		lineTerminatorCharacters: ["\r", "\n"],
		tracer: (e, t) => t()
	});
	let n = t.tracer;
	n("initCharCodeToOptimizedIndexMap", () => {
		fp();
	});
	let r;
	n("Reject Lexer.NA", () => {
		r = md(e, (e) => e[jf] === L.NA);
	});
	let i = !1, a;
	n("Transform Patterns", () => {
		i = !1, a = k(r, (e) => {
			let t = e[jf];
			if (Zu(t)) {
				let e = t.source;
				return e.length === 1 && e !== "^" && e !== "$" && e !== "." && !t.ignoreCase ? e : e.length === 2 && e[0] === "\\" && !zu([
					"d",
					"D",
					"s",
					"S",
					"t",
					"r",
					"n",
					"t",
					"0",
					"c",
					"b",
					"B",
					"f",
					"v",
					"w",
					"W"
				], e[1]) ? e[1] : Qf(t);
			}
			if (Ie(t)) return i = !0, { exec: t };
			if (typeof t == "object") return i = !0, t;
			if (typeof t == "string") {
				if (t.length === 1) return t;
				{
					let e = t.replace(/[\\^$.*+?()[\]{}|]/g, "\\$&");
					return Qf(new RegExp(e));
				}
			}
			throw Error("non exhaustive match");
		});
	});
	let o, s, c, l, u;
	n("misc mapping", () => {
		o = k(r, (e) => e.tokenTypeIdx), s = k(r, (e) => {
			let t = e.GROUP;
			if (t !== L.SKIPPED) {
				if (Nu(t)) return t;
				if ($u(t)) return !1;
				throw Error("non exhaustive match");
			}
		}), c = k(r, (e) => {
			let t = e.LONGER_ALT;
			if (t) return C(t) ? k(t, (e) => Hu(r, e)) : [Hu(r, t)];
		}), l = k(r, (e) => e.PUSH_MODE), u = k(r, (e) => A(e, "POP_MODE"));
	});
	let d;
	n("Line Terminator Handling", () => {
		let e = sp(t.lineTerminatorCharacters);
		d = k(r, (e) => !1), t.positionTracking !== "onlyOffset" && (d = k(r, (t) => A(t, "LINE_BREAKS") ? !!t.LINE_BREAKS : ap(t, e) === !1 && Af(e, t.PATTERN)));
	});
	let f, p, m, h;
	n("Misc Mapping #2", () => {
		f = k(r, np), p = k(a, rp), m = fd(r, (e, t) => {
			let n = t.GROUP;
			return Nu(n) && n !== L.SKIPPED && (e[n] = []), e;
		}, {}), h = k(a, (e, t) => ({
			pattern: a[t],
			longerAlt: c[t],
			canLineTerminator: d[t],
			isCustom: f[t],
			short: p[t],
			group: s[t],
			push: l[t],
			pop: u[t],
			tokenTypeIdx: o[t],
			tokenType: r[t]
		}));
	});
	let g = !0, _ = [];
	return t.safeMode || n("First Char Optimization", () => {
		_ = fd(r, (e, n, r) => {
			if (typeof n.PATTERN == "string") cp(e, dp(n.PATTERN.charCodeAt(0)), h[r]);
			else if (C(n.START_CHARS_HINT)) {
				let t;
				O(n.START_CHARS_HINT, (n) => {
					let i = dp(typeof n == "string" ? n.charCodeAt(0) : n);
					t !== i && (t = i, cp(e, i, h[r]));
				});
			} else if (Zu(n.PATTERN)) {
				if (n.PATTERN.unicode) g = !1, t.ensureOptimizations && Td(`${Sf}	Unable to analyze < ${n.PATTERN.toString()} > pattern.
	The regexp unicode flag is not currently supported by the regexp-to-ast library.
	This will disable the lexer's first char optimizations.
	For details See: https://chevrotain.io/docs/guide/resolving_lexer_errors.html#UNICODE_OPTIMIZE`);
				else {
					let i = Cf(n.PATTERN, t.ensureOptimizations);
					M(i) && (g = !1), O(i, (t) => {
						cp(e, t, h[r]);
					});
				}
			} else t.ensureOptimizations && Td(`${Sf}	TokenType: <${n.name}> is using a custom token pattern without providing <start_chars_hint> parameter.
	This will disable the lexer's first char optimizations.
	For details See: https://chevrotain.io/docs/guide/resolving_lexer_errors.html#CUSTOM_OPTIMIZE`), g = !1;
			return e;
		}, []);
	}), {
		emptyGroups: m,
		patternIdxToConfig: h,
		charCodeToPatternIdxToConfig: _,
		hasCustom: i,
		canBeOptimized: g
	};
}
function Ff(e, t) {
	let n = [], r = Lf(e);
	n = n.concat(r.errors);
	let i = Rf(r.valid), a = i.valid;
	return n = n.concat(i.errors), n = n.concat(If(a)), n = n.concat(Kf(a)), n = n.concat(qf(a, t)), n = n.concat(Jf(a)), n;
}
function If(e) {
	let t = [], n = fu(e, (e) => Zu(e[jf]));
	return t = t.concat(Bf(n)), t = t.concat(Uf(n)), t = t.concat(Wf(n)), t = t.concat(Gf(n)), t = t.concat(Vf(n)), t;
}
function Lf(e) {
	let t = fu(e, (e) => !A(e, jf));
	return {
		errors: k(t, (e) => ({
			message: "Token Type: ->" + e.name + "<- missing static 'PATTERN' property",
			type: I.MISSING_PATTERN,
			tokenTypes: [e]
		})),
		valid: Jl(e, t)
	};
}
function Rf(e) {
	let t = fu(e, (e) => {
		let t = e[jf];
		return !Zu(t) && !Ie(t) && !A(t, "exec") && !Nu(t);
	});
	return {
		errors: k(t, (e) => ({
			message: "Token Type: ->" + e.name + "<- static 'PATTERN' can only be a RegExp, a Function matching the {CustomPatternMatcherFunc} type or an Object matching the {ICustomPattern} interface.",
			type: I.INVALID_PATTERN,
			tokenTypes: [e]
		})),
		valid: Jl(e, t)
	};
}
var zf = /[^\\][$]/;
function Bf(e) {
	class t extends gf {
		constructor() {
			super(...arguments), this.found = !1;
		}
		visitEndAnchor(e) {
			this.found = !0;
		}
	}
	return k(fu(e, (e) => {
		let n = e.PATTERN;
		try {
			let e = yf(n), r = new t();
			return r.visit(e), r.found;
		} catch {
			return zf.test(n.source);
		}
	}), (e) => ({
		message: "Unexpected RegExp Anchor Error:\n	Token Type: ->" + e.name + "<- static 'PATTERN' cannot contain end of input anchor '$'\n	See chevrotain.io/docs/guide/resolving_lexer_errors.html#ANCHORS	for details.",
		type: I.EOI_ANCHOR_FOUND,
		tokenTypes: [e]
	}));
}
function Vf(e) {
	return k(fu(e, (e) => e.PATTERN.test("")), (e) => ({
		message: "Token Type: ->" + e.name + "<- static 'PATTERN' must not match an empty string",
		type: I.EMPTY_MATCH_PATTERN,
		tokenTypes: [e]
	}));
}
var Hf = /[^\\[][\^]|^\^/;
function Uf(e) {
	class t extends gf {
		constructor() {
			super(...arguments), this.found = !1;
		}
		visitStartAnchor(e) {
			this.found = !0;
		}
	}
	return k(fu(e, (e) => {
		let n = e.PATTERN;
		try {
			let e = yf(n), r = new t();
			return r.visit(e), r.found;
		} catch {
			return Hf.test(n.source);
		}
	}), (e) => ({
		message: "Unexpected RegExp Anchor Error:\n	Token Type: ->" + e.name + "<- static 'PATTERN' cannot contain start of input anchor '^'\n	See https://chevrotain.io/docs/guide/resolving_lexer_errors.html#ANCHORS	for details.",
		type: I.SOI_ANCHOR_FOUND,
		tokenTypes: [e]
	}));
}
function Wf(e) {
	return k(fu(e, (e) => {
		let t = e[jf];
		return t instanceof RegExp && (t.multiline || t.global);
	}), (e) => ({
		message: "Token Type: ->" + e.name + "<- static 'PATTERN' may NOT contain global('g') or multiline('m')",
		type: I.UNSUPPORTED_FLAGS_FOUND,
		tokenTypes: [e]
	}));
}
function Gf(e) {
	let t = [], n = k(e, (n) => fd(e, (e, r) => n.PATTERN.source === r.PATTERN.source && !zu(t, r) && r.PATTERN !== L.NA ? (t.push(r), e.push(r), e) : e, []));
	return n = ec(n), k(fu(n, (e) => e.length > 1), (e) => {
		let t = k(e, (e) => e.name);
		return {
			message: `The same RegExp pattern ->${yu(e).PATTERN}<-has been used in all of the following Token Types: ${t.join(", ")} <-`,
			type: I.DUPLICATE_PATTERNS_FOUND,
			tokenTypes: e
		};
	});
}
function Kf(e) {
	return k(fu(e, (e) => {
		if (!A(e, "GROUP")) return !1;
		let t = e.GROUP;
		return t !== L.SKIPPED && t !== L.NA && !Nu(t);
	}), (e) => ({
		message: "Token Type: ->" + e.name + "<- static 'GROUP' can only be Lexer.SKIPPED/Lexer.NA/A String",
		type: I.INVALID_GROUP_TYPE_FOUND,
		tokenTypes: [e]
	}));
}
function qf(e, t) {
	return k(fu(e, (e) => e.PUSH_MODE !== void 0 && !zu(t, e.PUSH_MODE)), (e) => ({
		message: `Token Type: ->${e.name}<- static 'PUSH_MODE' value cannot refer to a Lexer Mode ->${e.PUSH_MODE}<-which does not exist`,
		type: I.PUSH_MODE_DOES_NOT_EXIST,
		tokenTypes: [e]
	}));
}
function Jf(e) {
	let t = [], n = fd(e, (e, t, n) => {
		let r = t.PATTERN;
		return r === L.NA || (Nu(r) ? e.push({
			str: r,
			idx: n,
			tokenType: t
		}) : Zu(r) && Xf(r) && e.push({
			str: r.source,
			idx: n,
			tokenType: t
		})), e;
	}, []);
	return O(e, (e, r) => {
		O(n, ({ str: n, idx: i, tokenType: a }) => {
			if (r < i && Yf(n, e.PATTERN)) {
				let n = `Token: ->${a.name}<- can never be matched.
Because it appears AFTER the Token Type ->${e.name}<-in the lexer's definition.
See https://chevrotain.io/docs/guide/resolving_lexer_errors.html#UNREACHABLE`;
				t.push({
					message: n,
					type: I.UNREACHABLE_PATTERN,
					tokenTypes: [e, a]
				});
			}
		});
	}), t;
}
function Yf(e, t) {
	if (Zu(t)) {
		if (Zf(t)) return !1;
		let n = t.exec(e);
		return n !== null && n.index === 0;
	}
	if (Ie(t)) return t(e, 0, [], {});
	if (A(t, "exec")) return t.exec(e, 0, [], {});
	if (typeof t == "string") return t === e;
	throw Error("non exhaustive match");
}
function Xf(e) {
	return _u([
		".",
		"\\",
		"[",
		"]",
		"|",
		"^",
		"$",
		"(",
		")",
		"?",
		"*",
		"+",
		"{"
	], (t) => e.source.indexOf(t) !== -1) === void 0;
}
function Zf(e) {
	return /(\(\?=)|(\(\?!)|(\(\?<=)|(\(\?<!)/.test(e.source);
}
function Qf(e) {
	let t = e.ignoreCase ? "iy" : "y";
	return RegExp(`${e.source}`, t);
}
function $f(e, t, n) {
	let r = [];
	return A(e, Mf) || r.push({
		message: "A MultiMode Lexer cannot be initialized without a <" + Mf + "> property in its definition\n",
		type: I.MULTI_MODE_LEXER_WITHOUT_DEFAULT_MODE
	}), A(e, Nf) || r.push({
		message: "A MultiMode Lexer cannot be initialized without a <" + Nf + "> property in its definition\n",
		type: I.MULTI_MODE_LEXER_WITHOUT_MODES_PROPERTY
	}), A(e, Nf) && A(e, Mf) && !A(e.modes, e.defaultMode) && r.push({
		message: `A MultiMode Lexer cannot be initialized with a ${Mf}: <${e.defaultMode}>which does not exist
`,
		type: I.MULTI_MODE_LEXER_DEFAULT_MODE_VALUE_DOES_NOT_EXIST
	}), A(e, Nf) && O(e.modes, (e, t) => {
		O(e, (n, i) => {
			$u(n) ? r.push({
				message: `A Lexer cannot be initialized using an undefined Token Type. Mode:<${t}> at index: <${i}>
`,
				type: I.LEXER_DEFINITION_CANNOT_CONTAIN_UNDEFINED
			}) : A(n, "LONGER_ALT") && O(C(n.LONGER_ALT) ? n.LONGER_ALT : [n.LONGER_ALT], (i) => {
				!$u(i) && !zu(e, i) && r.push({
					message: `A MultiMode Lexer cannot be initialized with a longer_alt <${i.name}> on token <${n.name}> outside of mode <${t}>
`,
					type: I.MULTI_MODE_LEXER_LONGER_ALT_NOT_IN_CURRENT_MODE
				});
			});
		});
	}), r;
}
function ep(e, t, n) {
	let r = [], i = !1, a = md(ec(Zi(j(e.modes))), (e) => e[jf] === L.NA), o = sp(n);
	return t && O(a, (e) => {
		let t = ap(e, o);
		if (t !== !1) {
			let n = {
				message: op(e, t),
				type: t.issue,
				tokenType: e
			};
			r.push(n);
		} else A(e, "LINE_BREAKS") ? e.LINE_BREAKS === !0 && (i = !0) : Af(o, e.PATTERN) && (i = !0);
	}), t && !i && r.push({
		message: "Warning: No LINE_BREAKS Found.\n	This Lexer has been defined to track line and column information,\n	But none of the Token Types can be identified as matching a line terminator.\n	See https://chevrotain.io/docs/guide/resolving_lexer_errors.html#LINE_BREAKS \n	for details.",
		type: I.NO_LINE_BREAKS_FLAGS
	}), r;
}
function tp(e) {
	let t = {};
	return O(mr(e), (n) => {
		let r = e[n];
		if (C(r)) t[n] = [];
		else throw Error("non exhaustive match");
	}), t;
}
function np(e) {
	let t = e.PATTERN;
	if (Zu(t)) return !1;
	if (Ie(t) || A(t, "exec")) return !0;
	if (Nu(t)) return !1;
	throw Error("non exhaustive match");
}
function rp(e) {
	return Nu(e) && e.length === 1 ? e.charCodeAt(0) : !1;
}
var ip = {
	test: function(e) {
		let t = e.length;
		for (let n = this.lastIndex; n < t; n++) {
			let t = e.charCodeAt(n);
			if (t === 10) return this.lastIndex = n + 1, !0;
			if (t === 13) return this.lastIndex = e.charCodeAt(n + 1) === 10 ? n + 2 : n + 1, !0;
		}
		return !1;
	},
	lastIndex: 0
};
function ap(e, t) {
	if (A(e, "LINE_BREAKS")) return !1;
	if (Zu(e.PATTERN)) {
		try {
			Af(t, e.PATTERN);
		} catch (e) {
			return {
				issue: I.IDENTIFY_TERMINATOR,
				errMsg: e.message
			};
		}
		return !1;
	}
	if (Nu(e.PATTERN)) return !1;
	if (np(e)) return { issue: I.CUSTOM_LINE_BREAK };
	throw Error("non exhaustive match");
}
function op(e, t) {
	if (t.issue === I.IDENTIFY_TERMINATOR) return `Warning: unable to identify line terminator usage in pattern.
	The problem is in the <${e.name}> Token Type
	 Root cause: ${t.errMsg}.
	For details See: https://chevrotain.io/docs/guide/resolving_lexer_errors.html#IDENTIFY_TERMINATOR`;
	if (t.issue === I.CUSTOM_LINE_BREAK) return `Warning: A Custom Token Pattern should specify the <line_breaks> option.
	The problem is in the <${e.name}> Token Type
	For details See: https://chevrotain.io/docs/guide/resolving_lexer_errors.html#CUSTOM_LINE_BREAK`;
	throw Error("non exhaustive match");
}
function sp(e) {
	return k(e, (e) => Nu(e) ? e.charCodeAt(0) : e);
}
function cp(e, t, n) {
	e[t] === void 0 ? e[t] = [n] : e[t].push(n);
}
var lp = 256, up = [];
function dp(e) {
	return e < lp ? e : up[e];
}
function fp() {
	if (M(up)) {
		up = Array(65536);
		for (let e = 0; e < 65536; e++) up[e] = e > 255 ? 255 + ~~(e / 255) : e;
	}
}
function pp(e, t) {
	let n = e.tokenTypeIdx;
	return n === t.tokenTypeIdx || t.isParent === !0 && t.categoryMatchesMap[n] === !0;
}
function mp(e, t) {
	return e.tokenTypeIdx === t.tokenTypeIdx;
}
var hp = 1, gp = {};
function _p(e) {
	let t = vp(e);
	yp(t), xp(t), bp(t), O(t, (e) => {
		e.isParent = e.categoryMatches.length > 0;
	});
}
function vp(e) {
	let t = Qs(e), n = e, r = !0;
	for (; r;) {
		n = ec(Zi(k(n, (e) => e.CATEGORIES)));
		let e = Jl(n, t);
		t = t.concat(e), M(e) ? r = !1 : n = e;
	}
	return t;
}
function yp(e) {
	O(e, (e) => {
		Cp(e) || (gp[hp] = e, e.tokenTypeIdx = hp++), wp(e) && !C(e.CATEGORIES) && (e.CATEGORIES = [e.CATEGORIES]), wp(e) || (e.CATEGORIES = []), Tp(e) || (e.categoryMatches = []), Ep(e) || (e.categoryMatchesMap = {});
	});
}
function bp(e) {
	O(e, (e) => {
		e.categoryMatches = [], O(e.categoryMatchesMap, (t, n) => {
			e.categoryMatches.push(gp[n].tokenTypeIdx);
		});
	});
}
function xp(e) {
	O(e, (e) => {
		Sp([], e);
	});
}
function Sp(e, t) {
	O(e, (e) => {
		t.categoryMatchesMap[e.tokenTypeIdx] = !0;
	}), O(t.CATEGORIES, (n) => {
		let r = e.concat(t);
		zu(r, n) || Sp(r, n);
	});
}
function Cp(e) {
	return A(e, "tokenTypeIdx");
}
function wp(e) {
	return A(e, "CATEGORIES");
}
function Tp(e) {
	return A(e, "categoryMatches");
}
function Ep(e) {
	return A(e, "categoryMatchesMap");
}
function Dp(e) {
	return A(e, "tokenTypeIdx");
}
var Op = {
	buildUnableToPopLexerModeMessage(e) {
		return `Unable to pop Lexer Mode after encountering Token ->${e.image}<- The Mode Stack is empty`;
	},
	buildUnexpectedCharactersMessage(e, t, n, r, i, a) {
		return `unexpected character: ->${e.charAt(t)}<- at offset: ${t}, skipped ${n} characters.`;
	}
}, I;
(function(e) {
	e[e.MISSING_PATTERN = 0] = "MISSING_PATTERN", e[e.INVALID_PATTERN = 1] = "INVALID_PATTERN", e[e.EOI_ANCHOR_FOUND = 2] = "EOI_ANCHOR_FOUND", e[e.UNSUPPORTED_FLAGS_FOUND = 3] = "UNSUPPORTED_FLAGS_FOUND", e[e.DUPLICATE_PATTERNS_FOUND = 4] = "DUPLICATE_PATTERNS_FOUND", e[e.INVALID_GROUP_TYPE_FOUND = 5] = "INVALID_GROUP_TYPE_FOUND", e[e.PUSH_MODE_DOES_NOT_EXIST = 6] = "PUSH_MODE_DOES_NOT_EXIST", e[e.MULTI_MODE_LEXER_WITHOUT_DEFAULT_MODE = 7] = "MULTI_MODE_LEXER_WITHOUT_DEFAULT_MODE", e[e.MULTI_MODE_LEXER_WITHOUT_MODES_PROPERTY = 8] = "MULTI_MODE_LEXER_WITHOUT_MODES_PROPERTY", e[e.MULTI_MODE_LEXER_DEFAULT_MODE_VALUE_DOES_NOT_EXIST = 9] = "MULTI_MODE_LEXER_DEFAULT_MODE_VALUE_DOES_NOT_EXIST", e[e.LEXER_DEFINITION_CANNOT_CONTAIN_UNDEFINED = 10] = "LEXER_DEFINITION_CANNOT_CONTAIN_UNDEFINED", e[e.SOI_ANCHOR_FOUND = 11] = "SOI_ANCHOR_FOUND", e[e.EMPTY_MATCH_PATTERN = 12] = "EMPTY_MATCH_PATTERN", e[e.NO_LINE_BREAKS_FLAGS = 13] = "NO_LINE_BREAKS_FLAGS", e[e.UNREACHABLE_PATTERN = 14] = "UNREACHABLE_PATTERN", e[e.IDENTIFY_TERMINATOR = 15] = "IDENTIFY_TERMINATOR", e[e.CUSTOM_LINE_BREAK = 16] = "CUSTOM_LINE_BREAK", e[e.MULTI_MODE_LEXER_LONGER_ALT_NOT_IN_CURRENT_MODE = 17] = "MULTI_MODE_LEXER_LONGER_ALT_NOT_IN_CURRENT_MODE";
})(I ||= {});
var kp = {
	deferDefinitionErrorsHandling: !1,
	positionTracking: "full",
	lineTerminatorsPattern: /\n|\r\n?/g,
	lineTerminatorCharacters: ["\n", "\r"],
	ensureOptimizations: !1,
	safeMode: !1,
	errorMessageProvider: Op,
	traceInitPerf: !1,
	skipValidations: !1,
	recoveryEnabled: !0
};
Object.freeze(kp);
var L = class {
	constructor(e, t = kp) {
		if (this.lexerDefinition = e, this.lexerDefinitionErrors = [], this.lexerDefinitionWarning = [], this.patternIdxToConfig = {}, this.charCodeToPatternIdxToConfig = {}, this.modes = [], this.emptyGroups = {}, this.trackStartLines = !0, this.trackEndLines = !0, this.hasCustom = !1, this.canModeBeOptimized = {}, this.TRACE_INIT = (e, t) => {
			if (this.traceInitPerf === !0) {
				this.traceInitIndent++;
				let n = Array(this.traceInitIndent + 1).join("	");
				this.traceInitIndent < this.traceInitMaxIdent && console.log(`${n}--> <${e}>`);
				let { time: r, value: i } = Dd(t), a = r > 10 ? console.warn : console.log;
				return this.traceInitIndent < this.traceInitMaxIdent && a(`${n}<-- <${e}> time: ${r}ms`), this.traceInitIndent--, i;
			}
			return t();
		}, typeof t == "boolean") throw Error("The second argument to the Lexer constructor is now an ILexerConfig Object.\na boolean 2nd argument is no longer supported");
		this.config = gr({}, kp, t);
		let n = this.config.traceInitPerf;
		n === !0 ? (this.traceInitMaxIdent = Infinity, this.traceInitPerf = !0) : typeof n == "number" && (this.traceInitMaxIdent = n, this.traceInitPerf = !0), this.traceInitIndent = -1, this.TRACE_INIT("Lexer Constructor", () => {
			let n, r = !0;
			this.TRACE_INIT("Lexer Config handling", () => {
				if (this.config.lineTerminatorsPattern === kp.lineTerminatorsPattern) this.config.lineTerminatorsPattern = ip;
				else if (this.config.lineTerminatorCharacters === kp.lineTerminatorCharacters) throw Error("Error: Missing <lineTerminatorCharacters> property on the Lexer config.\n	For details See: https://chevrotain.io/docs/guide/resolving_lexer_errors.html#MISSING_LINE_TERM_CHARS");
				if (t.safeMode && t.ensureOptimizations) throw Error("\"safeMode\" and \"ensureOptimizations\" flags are mutually exclusive.");
				this.trackStartLines = /full|onlyStart/i.test(this.config.positionTracking), this.trackEndLines = /full/i.test(this.config.positionTracking), C(e) ? n = {
					modes: { defaultMode: Qs(e) },
					defaultMode: Mf
				} : (r = !1, n = Qs(e));
			}), this.config.skipValidations === !1 && (this.TRACE_INIT("performRuntimeChecks", () => {
				this.lexerDefinitionErrors = this.lexerDefinitionErrors.concat($f(n, this.trackStartLines, this.config.lineTerminatorCharacters));
			}), this.TRACE_INIT("performWarningRuntimeChecks", () => {
				this.lexerDefinitionWarning = this.lexerDefinitionWarning.concat(ep(n, this.trackStartLines, this.config.lineTerminatorCharacters));
			})), n.modes = n.modes ? n.modes : {}, O(n.modes, (e, t) => {
				n.modes[t] = md(e, (e) => $u(e));
			});
			let i = mr(n.modes);
			if (O(n.modes, (e, n) => {
				this.TRACE_INIT(`Mode: <${n}> processing`, () => {
					if (this.modes.push(n), this.config.skipValidations === !1 && this.TRACE_INIT("validatePatterns", () => {
						this.lexerDefinitionErrors = this.lexerDefinitionErrors.concat(Ff(e, i));
					}), M(this.lexerDefinitionErrors)) {
						_p(e);
						let r;
						this.TRACE_INIT("analyzeTokenTypes", () => {
							r = Pf(e, {
								lineTerminatorCharacters: this.config.lineTerminatorCharacters,
								positionTracking: t.positionTracking,
								ensureOptimizations: t.ensureOptimizations,
								safeMode: t.safeMode,
								tracer: this.TRACE_INIT
							});
						}), this.patternIdxToConfig[n] = r.patternIdxToConfig, this.charCodeToPatternIdxToConfig[n] = r.charCodeToPatternIdxToConfig, this.emptyGroups = gr({}, this.emptyGroups, r.emptyGroups), this.hasCustom = r.hasCustom || this.hasCustom, this.canModeBeOptimized[n] = r.canBeOptimized;
					}
				});
			}), this.defaultMode = n.defaultMode, !M(this.lexerDefinitionErrors) && !this.config.deferDefinitionErrorsHandling) {
				let e = k(this.lexerDefinitionErrors, (e) => e.message).join("-----------------------\n");
				throw Error("Errors detected in definition of Lexer:\n" + e);
			}
			O(this.lexerDefinitionWarning, (e) => {
				Ed(e.message);
			}), this.TRACE_INIT("Choosing sub-methods implementations", () => {
				if (r && (this.handleModes = w), this.trackStartLines === !1 && (this.computeNewColumn = Ae), this.trackEndLines === !1 && (this.updateTokenEndLineColumnLocation = w), /full/i.test(this.config.positionTracking)) this.createTokenInstance = this.createFullToken;
				else if (/onlyStart/i.test(this.config.positionTracking)) this.createTokenInstance = this.createStartOnlyToken;
				else if (/onlyOffset/i.test(this.config.positionTracking)) this.createTokenInstance = this.createOffsetOnlyToken;
				else throw Error(`Invalid <positionTracking> config option: "${this.config.positionTracking}"`);
				this.hasCustom ? (this.addToken = this.addTokenUsingPush, this.handlePayload = this.handlePayloadWithCustom) : (this.addToken = this.addTokenUsingMemberAccess, this.handlePayload = this.handlePayloadNoCustom);
			}), this.TRACE_INIT("Failed Optimization Warnings", () => {
				let e = fd(this.canModeBeOptimized, (e, t, n) => (t === !1 && e.push(n), e), []);
				if (t.ensureOptimizations && !M(e)) throw Error(`Lexer Modes: < ${e.join(", ")} > cannot be optimized.
	 Disable the "ensureOptimizations" lexer config flag to silently ignore this and run the lexer in an un-optimized mode.
	 Or inspect the console log for details on how to resolve these issues.`);
			}), this.TRACE_INIT("clearRegExpParserCache", () => {
				bf();
			}), this.TRACE_INIT("toFastProperties", () => {
				Od(this);
			});
		});
	}
	tokenize(e, t = this.defaultMode) {
		if (!M(this.lexerDefinitionErrors)) {
			let e = k(this.lexerDefinitionErrors, (e) => e.message).join("-----------------------\n");
			throw Error("Unable to Tokenize because Errors detected in definition of Lexer:\n" + e);
		}
		return this.tokenizeInternal(e, t);
	}
	tokenizeInternal(e, t) {
		let n, r, i, a, o, s, c, l, u, d, f, p, m, h, g, _ = e, v = _.length, y = 0, b = 0, ee = this.hasCustom ? 0 : Math.floor(e.length / 10), x = Array(ee), te = [], ne = this.trackStartLines ? 1 : void 0, S = this.trackStartLines ? 1 : void 0, C = tp(this.emptyGroups), re = this.trackStartLines, ie = this.config.lineTerminatorsPattern, ae = 0, oe = [], se = [], ce = [], le = [];
		Object.freeze(le);
		let ue = !1, de = (e) => {
			if (ce.length === 1 && e.tokenType.PUSH_MODE === void 0) {
				let t = this.config.errorMessageProvider.buildUnableToPopLexerModeMessage(e);
				te.push({
					offset: e.startOffset,
					line: e.startLine,
					column: e.startColumn,
					length: e.image.length,
					message: t
				});
			} else {
				ce.pop();
				let e = Xl(ce);
				oe = this.patternIdxToConfig[e], se = this.charCodeToPatternIdxToConfig[e], ae = oe.length;
				let t = this.canModeBeOptimized[e] && this.config.safeMode === !1;
				ue = !!(se && t);
			}
		};
		function fe(e) {
			ce.push(e), se = this.charCodeToPatternIdxToConfig[e], oe = this.patternIdxToConfig[e], ae = oe.length, ae = oe.length;
			let t = this.canModeBeOptimized[e] && this.config.safeMode === !1;
			ue = !!(se && t);
		}
		fe.call(this, t);
		let pe, me = this.config.recoveryEnabled;
		for (; y < v;) {
			s = null, u = -1;
			let t = _.charCodeAt(y), ee;
			if (ue) {
				let e = dp(t), n = se[e];
				ee = n === void 0 ? le : n;
			} else ee = oe;
			let he = ee.length;
			for (n = 0; n < he; n++) {
				pe = ee[n];
				let r = pe.pattern;
				c = null;
				let d = pe.short;
				if (d === !1 ? pe.isCustom === !0 ? (g = r.exec(_, y, x, C), g === null ? s = null : (s = g[0], u = s.length, g.payload !== void 0 && (c = g.payload))) : (r.lastIndex = y, u = this.matchLength(r, e, y)) : t === d && (u = 1, s = r), u !== -1) {
					if (o = pe.longerAlt, o !== void 0) {
						s = e.substring(y, y + u);
						let t = o.length;
						for (i = 0; i < t; i++) {
							let t = oe[o[i]], n = t.pattern;
							if (l = null, t.isCustom === !0 ? (g = n.exec(_, y, x, C), g === null ? a = null : (a = g[0], g.payload !== void 0 && (l = g.payload))) : (n.lastIndex = y, a = this.match(n, e, y)), a && a.length > s.length) {
								s = a, u = a.length, c = l, pe = t;
								break;
							}
						}
					}
					break;
				}
			}
			if (u !== -1) {
				if (d = pe.group, d !== void 0 && (s = s === null ? e.substring(y, y + u) : s, f = pe.tokenTypeIdx, p = this.createTokenInstance(s, y, f, pe.tokenType, ne, S, u), this.handlePayload(p, c), d === !1 ? b = this.addToken(x, b, p) : C[d].push(p)), re === !0 && pe.canLineTerminator === !0) {
					let t = 0, n, r;
					ie.lastIndex = 0;
					do
						s = s === null ? e.substring(y, y + u) : s, n = ie.test(s), n === !0 && (r = ie.lastIndex - 1, t++);
					while (n === !0);
					t === 0 ? S = this.computeNewColumn(S, u) : (ne += t, S = u - r, this.updateTokenEndLineColumnLocation(p, d, r, t, ne, S, u));
				} else S = this.computeNewColumn(S, u);
				y += u, this.handleModes(pe, de, fe, p);
			} else {
				let t = y, n = ne, i = S, a = me === !1;
				for (; a === !1 && y < v;) for (y++, r = 0; r < ae; r++) {
					let t = oe[r], n = t.pattern, i = t.short;
					if (i === !1 ? t.isCustom === !0 ? a = n.exec(_, y, x, C) !== null : (n.lastIndex = y, a = n.exec(e) !== null) : _.charCodeAt(y) === i && (a = !0), a === !0) break;
				}
				if (m = y - t, S = this.computeNewColumn(S, m), h = this.config.errorMessageProvider.buildUnexpectedCharactersMessage(_, t, m, n, i, Xl(ce)), te.push({
					offset: t,
					line: n,
					column: i,
					length: m,
					message: h
				}), me === !1) break;
			}
		}
		return this.hasCustom || (x.length = b), {
			tokens: x,
			groups: C,
			errors: te
		};
	}
	handleModes(e, t, n, r) {
		if (e.pop === !0) {
			let i = e.push;
			t(r), i !== void 0 && n.call(this, i);
		} else e.push !== void 0 && n.call(this, e.push);
	}
	updateTokenEndLineColumnLocation(e, t, n, r, i, a, o) {
		let s, c;
		t !== void 0 && (s = n === o - 1, c = s ? -1 : 0, (r !== 1 || s !== !0) && (e.endLine = i + c, e.endColumn = a - 1 + -c));
	}
	computeNewColumn(e, t) {
		return e + t;
	}
	createOffsetOnlyToken(e, t, n, r) {
		return {
			image: e,
			startOffset: t,
			tokenTypeIdx: n,
			tokenType: r
		};
	}
	createStartOnlyToken(e, t, n, r, i, a) {
		return {
			image: e,
			startOffset: t,
			startLine: i,
			startColumn: a,
			tokenTypeIdx: n,
			tokenType: r
		};
	}
	createFullToken(e, t, n, r, i, a, o) {
		return {
			image: e,
			startOffset: t,
			endOffset: t + o - 1,
			startLine: i,
			endLine: i,
			startColumn: a,
			endColumn: a + o - 1,
			tokenTypeIdx: n,
			tokenType: r
		};
	}
	addTokenUsingPush(e, t, n) {
		return e.push(n), t;
	}
	addTokenUsingMemberAccess(e, t, n) {
		return e[t] = n, t++, t;
	}
	handlePayloadNoCustom(e, t) {}
	handlePayloadWithCustom(e, t) {
		t !== null && (e.payload = t);
	}
	match(e, t, n) {
		return e.test(t) === !0 ? t.substring(n, e.lastIndex) : null;
	}
	matchLength(e, t, n) {
		return e.test(t) === !0 ? e.lastIndex - n : -1;
	}
};
L.SKIPPED = "This marks a skipped Token pattern, this means each token identified by it will be consumed and then thrown into oblivion, this can be used to for example to completely ignore whitespace.", L.NA = /NOT_APPLICABLE/;
function Ap(e) {
	return jp(e) ? e.LABEL : e.name;
}
function jp(e) {
	return Nu(e.LABEL) && e.LABEL !== "";
}
var Mp = "parent", Np = "categories", Pp = "label", Fp = "group", Ip = "push_mode", Lp = "pop_mode", Rp = "longer_alt", zp = "line_breaks", Bp = "start_chars_hint";
function Vp(e) {
	return Hp(e);
}
function Hp(e) {
	let t = e.pattern, n = {};
	if (n.name = e.name, $u(t) || (n.PATTERN = t), A(e, Mp)) throw "The parent property is no longer supported.\nSee: https://github.com/chevrotain/chevrotain/issues/564#issuecomment-349062346 for details.";
	return A(e, Np) && (n.CATEGORIES = e[Np]), _p([n]), A(e, Pp) && (n.LABEL = e[Pp]), A(e, Fp) && (n.GROUP = e[Fp]), A(e, Lp) && (n.POP_MODE = e[Lp]), A(e, Ip) && (n.PUSH_MODE = e[Ip]), A(e, Rp) && (n.LONGER_ALT = e[Rp]), A(e, zp) && (n.LINE_BREAKS = e[zp]), A(e, Bp) && (n.START_CHARS_HINT = e[Bp]), n;
}
var Up = Vp({
	name: "EOF",
	pattern: L.NA
});
_p([Up]);
function Wp(e, t, n, r, i, a, o, s) {
	return {
		image: t,
		startOffset: n,
		endOffset: r,
		startLine: i,
		endLine: a,
		startColumn: o,
		endColumn: s,
		tokenTypeIdx: e.tokenTypeIdx,
		tokenType: e
	};
}
function Gp(e, t) {
	return pp(e, t);
}
var Kp = {
	buildMismatchTokenMessage({ expected: e, actual: t, previous: n, ruleName: r }) {
		return `Expecting ${jp(e) ? `--> ${Ap(e)} <--` : `token of type --> ${e.name} <--`} but found --> '${t.image}' <--`;
	},
	buildNotAllInputParsedMessage({ firstRedundant: e, ruleName: t }) {
		return "Redundant input, expecting EOF but found: " + e.image;
	},
	buildNoViableAltMessage({ expectedPathsPerAlt: e, actual: t, previous: n, customUserDescription: r, ruleName: i }) {
		let a = "\nbut found: '" + yu(t).image + "'";
		return r ? "Expecting: " + r + a : `Expecting: one of these possible Token sequences:
${k(k(fd(e, (e, t) => e.concat(t), []), (e) => `[${k(e, (e) => Ap(e)).join(", ")}]`), (e, t) => `  ${t + 1}. ${e}`).join("\n")}` + a;
	},
	buildEarlyExitMessage({ expectedIterationPaths: e, actual: t, customUserDescription: n, ruleName: r }) {
		let i = "\nbut found: '" + yu(t).image + "'";
		return n ? "Expecting: " + n + i : `Expecting: expecting at least one iteration which starts with one of these possible Token sequences::
  <${k(e, (e) => `[${k(e, (e) => Ap(e)).join(",")}]`).join(" ,")}>` + i;
	}
};
Object.freeze(Kp);
var qp = { buildRuleNotFoundError(e, t) {
	return "Invalid grammar, reference to a rule which is not defined: ->" + t.nonTerminalName + "<-\ninside top level rule: ->" + e.name + "<-";
} }, Jp = {
	buildDuplicateFoundError(e, t) {
		function n(e) {
			return e instanceof P ? e.terminalType.name : e instanceof Md ? e.nonTerminalName : "";
		}
		let r = e.name, i = yu(t), a = i.idx, o = Kd(i), s = n(i), c = `->${o}${a > 0 ? a : ""}<- ${s ? `with argument: ->${s}<-` : ""}
                  appears more than once (${t.length} times) in the top level rule: ->${r}<-.                  
                  For further details see: https://chevrotain.io/docs/FAQ.html#NUMERICAL_SUFFIXES 
                  `;
		return c = c.replace(/[ \t]+/g, " "), c = c.replace(/\s\s+/g, "\n"), c;
	},
	buildNamespaceConflictError(e) {
		return `Namespace conflict found in grammar.
The grammar has both a Terminal(Token) and a Non-Terminal(Rule) named: <${e.name}>.
To resolve this make sure each Terminal and Non-Terminal names are unique
This is easy to accomplish by using the convention that Terminal names start with an uppercase letter
and Non-Terminal names start with a lower case letter.`;
	},
	buildAlternationPrefixAmbiguityError(e) {
		let t = k(e.prefixPath, (e) => Ap(e)).join(", "), n = e.alternation.idx === 0 ? "" : e.alternation.idx;
		return `Ambiguous alternatives: <${e.ambiguityIndices.join(" ,")}> due to common lookahead prefix
in <OR${n}> inside <${e.topLevelRule.name}> Rule,
<${t}> may appears as a prefix path in all these alternatives.
See: https://chevrotain.io/docs/guide/resolving_grammar_errors.html#COMMON_PREFIX
For Further details.`;
	},
	buildAlternationAmbiguityError(e) {
		let t = e.alternation.idx === 0 ? "" : e.alternation.idx, n = e.prefixPath.length === 0, r = `Ambiguous Alternatives Detected: <${e.ambiguityIndices.join(" ,")}> in <OR${t}> inside <${e.topLevelRule.name}> Rule,
`;
		if (n) r += "These alternatives are all empty (match no tokens), making them indistinguishable.\nOnly the last alternative may be empty.\n";
		else {
			let t = k(e.prefixPath, (e) => Ap(e)).join(", ");
			r += `<${t}> may appears as a prefix path in all these alternatives.
`;
		}
		return r += "See: https://chevrotain.io/docs/guide/resolving_grammar_errors.html#AMBIGUOUS_ALTERNATIVES\nFor Further details.", r;
	},
	buildEmptyRepetitionError(e) {
		let t = Kd(e.repetition);
		return e.repetition.idx !== 0 && (t += e.repetition.idx), `The repetition <${t}> within Rule <${e.topLevelRule.name}> can never consume any tokens.
This could lead to an infinite loop.`;
	},
	buildTokenNameError(e) {
		return "deprecated";
	},
	buildEmptyAlternationError(e) {
		return `Ambiguous empty alternative: <${e.emptyChoiceIdx + 1}> in <OR${e.alternation.idx}> inside <${e.topLevelRule.name}> Rule.
Only the last alternative may be an empty alternative.`;
	},
	buildTooManyAlternativesError(e) {
		return `An Alternation cannot have more than 256 alternatives:
<OR${e.alternation.idx}> inside <${e.topLevelRule.name}> Rule.
 has ${e.alternation.definition.length + 1} alternatives.`;
	},
	buildLeftRecursionError(e) {
		let t = e.topLevelRule.name;
		return `Left Recursion found in grammar.
rule: <${t}> can be invoked from itself (directly or indirectly)
without consuming any Tokens. The grammar path that causes this is: 
 ${`${t} --> ${k(e.leftRecursionPath, (e) => e.name).concat([t]).join(" --> ")}`}
 To fix this refactor your grammar to remove the left recursion.
see: https://en.wikipedia.org/wiki/LL_parser#Left_factoring.`;
	},
	buildInvalidRuleNameError(e) {
		return "deprecated";
	},
	buildDuplicateRuleNameError(e) {
		let t;
		return t = e.topLevelRule instanceof Nd ? e.topLevelRule.name : e.topLevelRule, `Duplicate definition, rule: ->${t}<- is already defined in the grammar: ->${e.grammarName}<-`;
	}
};
function Yp(e, t) {
	let n = new Xp(e, t);
	return n.resolveRefs(), n.errors;
}
var Xp = class extends Hd {
	constructor(e, t) {
		super(), this.nameToTopRule = e, this.errMsgProvider = t, this.errors = [];
	}
	resolveRefs() {
		O(j(this.nameToTopRule), (e) => {
			this.currTopLevel = e, e.accept(this);
		});
	}
	visitNonTerminal(e) {
		let t = this.nameToTopRule[e.nonTerminalName];
		if (t) e.referencedRule = t;
		else {
			let t = this.errMsgProvider.buildRuleNotFoundError(this.currTopLevel, e);
			this.errors.push({
				message: t,
				type: eg.UNRESOLVED_SUBRULE_REF,
				ruleName: this.currTopLevel.name,
				unresolvedRefName: e.nonTerminalName
			});
		}
	}
}, Zp = class extends qd {
	constructor(e, t) {
		super(), this.topProd = e, this.path = t, this.possibleTokTypes = [], this.nextProductionName = "", this.nextProductionOccurrence = 0, this.found = !1, this.isAtEndOfPath = !1;
	}
	startWalking() {
		if (this.found = !1, this.path.ruleStack[0] !== this.topProd.name) throw Error("The path does not start with the walker's top Rule!");
		return this.ruleStack = Qs(this.path.ruleStack).reverse(), this.occurrenceStack = Qs(this.path.occurrenceStack).reverse(), this.ruleStack.pop(), this.occurrenceStack.pop(), this.updateExpectedNext(), this.walk(this.topProd), this.possibleTokTypes;
	}
	walk(e, t = []) {
		this.found || super.walk(e, t);
	}
	walkProdRef(e, t, n) {
		if (e.referencedRule.name === this.nextProductionName && e.idx === this.nextProductionOccurrence) {
			let r = t.concat(n);
			this.updateExpectedNext(), this.walk(e.referencedRule, r);
		}
	}
	updateExpectedNext() {
		M(this.ruleStack) ? (this.nextProductionName = "", this.nextProductionOccurrence = 0, this.isAtEndOfPath = !0) : (this.nextProductionName = this.ruleStack.pop(), this.nextProductionOccurrence = this.occurrenceStack.pop());
	}
}, Qp = class extends Zp {
	constructor(e, t) {
		super(e, t), this.path = t, this.nextTerminalName = "", this.nextTerminalOccurrence = 0, this.nextTerminalName = this.path.lastTok.name, this.nextTerminalOccurrence = this.path.lastTokOccurrence;
	}
	walkTerminal(e, t, n) {
		if (this.isAtEndOfPath && e.terminalType.name === this.nextTerminalName && e.idx === this.nextTerminalOccurrence && !this.found) {
			let e = new Pd({ definition: t.concat(n) });
			this.possibleTokTypes = Yd(e), this.found = !0;
		}
	}
}, $p = class extends qd {
	constructor(e, t) {
		super(), this.topRule = e, this.occurrence = t, this.result = {
			token: void 0,
			occurrence: void 0,
			isEndOfRule: void 0
		};
	}
	startWalking() {
		return this.walk(this.topRule), this.result;
	}
}, em = class extends $p {
	walkMany(e, t, n) {
		if (e.idx === this.occurrence) {
			let e = yu(t.concat(n));
			this.result.isEndOfRule = e === void 0, e instanceof P && (this.result.token = e.terminalType, this.result.occurrence = e.idx);
		} else super.walkMany(e, t, n);
	}
}, tm = class extends $p {
	walkManySep(e, t, n) {
		if (e.idx === this.occurrence) {
			let e = yu(t.concat(n));
			this.result.isEndOfRule = e === void 0, e instanceof P && (this.result.token = e.terminalType, this.result.occurrence = e.idx);
		} else super.walkManySep(e, t, n);
	}
}, nm = class extends $p {
	walkAtLeastOne(e, t, n) {
		if (e.idx === this.occurrence) {
			let e = yu(t.concat(n));
			this.result.isEndOfRule = e === void 0, e instanceof P && (this.result.token = e.terminalType, this.result.occurrence = e.idx);
		} else super.walkAtLeastOne(e, t, n);
	}
}, rm = class extends $p {
	walkAtLeastOneSep(e, t, n) {
		if (e.idx === this.occurrence) {
			let e = yu(t.concat(n));
			this.result.isEndOfRule = e === void 0, e instanceof P && (this.result.token = e.terminalType, this.result.occurrence = e.idx);
		} else super.walkAtLeastOneSep(e, t, n);
	}
};
function im(e, t, n = []) {
	n = Qs(n);
	let r = [], i = 0;
	function a(t) {
		return t.concat(D(e, i + 1));
	}
	function o(e) {
		let i = im(a(e), t, n);
		return r.concat(i);
	}
	for (; n.length < t && i < e.length;) {
		let t = e[i];
		if (t instanceof Pd || t instanceof Md) return o(t.definition);
		if (t instanceof Fd) r = o(t.definition);
		else if (t instanceof Id) return o(t.definition.concat([new N({ definition: t.definition })]));
		else if (t instanceof Ld) return o([new Pd({ definition: t.definition }), new N({ definition: [new P({ terminalType: t.separator })].concat(t.definition) })]);
		else if (t instanceof Rd) r = o(t.definition.concat([new N({ definition: [new P({ terminalType: t.separator })].concat(t.definition) })]));
		else if (t instanceof N) r = o(t.definition.concat([new N({ definition: t.definition })]));
		else if (t instanceof zd) return O(t.definition, (e) => {
			M(e.definition) === !1 && (r = o(e.definition));
		}), r;
		else if (t instanceof P) n.push(t.terminalType);
		else throw Error("non exhaustive match");
		i++;
	}
	return r.push({
		partialPath: n,
		suffixDef: D(e, i)
	}), r;
}
function am(e, t, n, r) {
	let i = "EXIT_NONE_TERMINAL", a = [i], o = "EXIT_ALTERNATIVE", s = !1, c = t.length, l = c - r - 1, u = [], d = [];
	for (d.push({
		idx: -1,
		def: e,
		ruleStack: [],
		occurrenceStack: []
	}); !M(d);) {
		let e = d.pop();
		if (e === o) {
			s && Xl(d).idx <= l && d.pop();
			continue;
		}
		let r = e.def, f = e.idx, p = e.ruleStack, m = e.occurrenceStack;
		if (M(r)) continue;
		let h = r[0];
		if (h === i) {
			let e = {
				idx: f,
				def: D(r),
				ruleStack: $l(p),
				occurrenceStack: $l(m)
			};
			d.push(e);
		} else if (h instanceof P) {
			if (f < c - 1) {
				let e = f + 1, i = t[e];
				if (n(i, h.terminalType)) {
					let t = {
						idx: e,
						def: D(r),
						ruleStack: p,
						occurrenceStack: m
					};
					d.push(t);
				}
			} else if (f === c - 1) u.push({
				nextTokenType: h.terminalType,
				nextTokenOccurrence: h.idx,
				ruleStack: p,
				occurrenceStack: m
			}), s = !0;
			else throw Error("non exhaustive match");
		} else if (h instanceof Md) {
			let e = Qs(p);
			e.push(h.nonTerminalName);
			let t = Qs(m);
			t.push(h.idx);
			let n = {
				idx: f,
				def: h.definition.concat(a, D(r)),
				ruleStack: e,
				occurrenceStack: t
			};
			d.push(n);
		} else if (h instanceof Fd) {
			let e = {
				idx: f,
				def: D(r),
				ruleStack: p,
				occurrenceStack: m
			};
			d.push(e), d.push(o);
			let t = {
				idx: f,
				def: h.definition.concat(D(r)),
				ruleStack: p,
				occurrenceStack: m
			};
			d.push(t);
		} else if (h instanceof Id) {
			let e = new N({
				definition: h.definition,
				idx: h.idx
			}), t = {
				idx: f,
				def: h.definition.concat([e], D(r)),
				ruleStack: p,
				occurrenceStack: m
			};
			d.push(t);
		} else if (h instanceof Ld) {
			let e = new N({
				definition: [new P({ terminalType: h.separator })].concat(h.definition),
				idx: h.idx
			}), t = {
				idx: f,
				def: h.definition.concat([e], D(r)),
				ruleStack: p,
				occurrenceStack: m
			};
			d.push(t);
		} else if (h instanceof Rd) {
			let e = {
				idx: f,
				def: D(r),
				ruleStack: p,
				occurrenceStack: m
			};
			d.push(e), d.push(o);
			let t = new N({
				definition: [new P({ terminalType: h.separator })].concat(h.definition),
				idx: h.idx
			}), n = {
				idx: f,
				def: h.definition.concat([t], D(r)),
				ruleStack: p,
				occurrenceStack: m
			};
			d.push(n);
		} else if (h instanceof N) {
			let e = {
				idx: f,
				def: D(r),
				ruleStack: p,
				occurrenceStack: m
			};
			d.push(e), d.push(o);
			let t = new N({
				definition: h.definition,
				idx: h.idx
			}), n = {
				idx: f,
				def: h.definition.concat([t], D(r)),
				ruleStack: p,
				occurrenceStack: m
			};
			d.push(n);
		} else if (h instanceof zd) for (let e = h.definition.length - 1; e >= 0; e--) {
			let t = {
				idx: f,
				def: h.definition[e].definition.concat(D(r)),
				ruleStack: p,
				occurrenceStack: m
			};
			d.push(t), d.push(o);
		}
		else if (h instanceof Pd) d.push({
			idx: f,
			def: h.definition.concat(D(r)),
			ruleStack: p,
			occurrenceStack: m
		});
		else if (h instanceof Nd) d.push(om(h, f, p, m));
		else throw Error("non exhaustive match");
	}
	return u;
}
function om(e, t, n, r) {
	let i = Qs(n);
	i.push(e.name);
	let a = Qs(r);
	return a.push(1), {
		idx: t,
		def: e.definition,
		ruleStack: i,
		occurrenceStack: a
	};
}
var R;
(function(e) {
	e[e.OPTION = 0] = "OPTION", e[e.REPETITION = 1] = "REPETITION", e[e.REPETITION_MANDATORY = 2] = "REPETITION_MANDATORY", e[e.REPETITION_MANDATORY_WITH_SEPARATOR = 3] = "REPETITION_MANDATORY_WITH_SEPARATOR", e[e.REPETITION_WITH_SEPARATOR = 4] = "REPETITION_WITH_SEPARATOR", e[e.ALTERNATION = 5] = "ALTERNATION";
})(R ||= {});
function sm(e) {
	if (e instanceof Fd || e === "Option") return R.OPTION;
	if (e instanceof N || e === "Repetition") return R.REPETITION;
	if (e instanceof Id || e === "RepetitionMandatory") return R.REPETITION_MANDATORY;
	if (e instanceof Ld || e === "RepetitionMandatoryWithSeparator") return R.REPETITION_MANDATORY_WITH_SEPARATOR;
	if (e instanceof Rd || e === "RepetitionWithSeparator") return R.REPETITION_WITH_SEPARATOR;
	if (e instanceof zd || e === "Alternation") return R.ALTERNATION;
	throw Error("non exhaustive match");
}
function cm(e, t, n, r, i, a) {
	let o = vm(e, t, n);
	return a(o, r, Sm(o) ? mp : pp, i);
}
function lm(e, t, n, r, i, a) {
	let o = ym(e, t, i, n), s = Sm(o) ? mp : pp;
	return a(o[0], s, r);
}
function um(e, t, n, r) {
	let i = e.length, a = cu(e, (e) => cu(e, (e) => e.length === 1));
	if (t) return function(t) {
		let r = k(t, (e) => e.GATE);
		for (let t = 0; t < i; t++) {
			let i = e[t], a = i.length, o = r[t];
			if (o === void 0 || o.call(this) !== !1) nextPath: for (let e = 0; e < a; e++) {
				let r = i[e], a = r.length;
				for (let e = 0; e < a; e++) if (n(this.LA_FAST(e + 1), r[e]) === !1) continue nextPath;
				return t;
			}
		}
	};
	if (a && !r) {
		let t = fd(k(e, (e) => Zi(e)), (e, t, n) => (O(t, (t) => {
			A(e, t.tokenTypeIdx) || (e[t.tokenTypeIdx] = n), O(t.categoryMatches, (t) => {
				A(e, t) || (e[t] = n);
			});
		}), e), {});
		return function() {
			let e = this.LA_FAST(1);
			return t[e.tokenTypeIdx];
		};
	}
	return function() {
		for (let t = 0; t < i; t++) {
			let r = e[t], i = r.length;
			nextPath: for (let e = 0; e < i; e++) {
				let i = r[e], a = i.length;
				for (let e = 0; e < a; e++) if (n(this.LA_FAST(e + 1), i[e]) === !1) continue nextPath;
				return t;
			}
		}
	};
}
function dm(e, t, n) {
	let r = cu(e, (e) => e.length === 1), i = e.length;
	if (r && !n) {
		let t = Zi(e);
		if (t.length === 1 && M(t[0].categoryMatches)) {
			let e = t[0].tokenTypeIdx;
			return function() {
				return this.LA_FAST(1).tokenTypeIdx === e;
			};
		}
		{
			let e = fd(t, (e, t, n) => (e[t.tokenTypeIdx] = !0, O(t.categoryMatches, (t) => {
				e[t] = !0;
			}), e), []);
			return function() {
				let t = this.LA_FAST(1);
				return e[t.tokenTypeIdx] === !0;
			};
		}
	}
	return function() {
		nextPath: for (let n = 0; n < i; n++) {
			let r = e[n], i = r.length;
			for (let e = 0; e < i; e++) if (t(this.LA_FAST(e + 1), r[e]) === !1) continue nextPath;
			return !0;
		}
		return !1;
	};
}
var fm = class extends qd {
	constructor(e, t, n) {
		super(), this.topProd = e, this.targetOccurrence = t, this.targetProdType = n;
	}
	startWalking() {
		return this.walk(this.topProd), this.restDef;
	}
	checkIsTarget(e, t, n, r) {
		return e.idx === this.targetOccurrence && this.targetProdType === t && (this.restDef = n.concat(r), !0);
	}
	walkOption(e, t, n) {
		this.checkIsTarget(e, R.OPTION, t, n) || super.walkOption(e, t, n);
	}
	walkAtLeastOne(e, t, n) {
		this.checkIsTarget(e, R.REPETITION_MANDATORY, t, n) || super.walkOption(e, t, n);
	}
	walkAtLeastOneSep(e, t, n) {
		this.checkIsTarget(e, R.REPETITION_MANDATORY_WITH_SEPARATOR, t, n) || super.walkOption(e, t, n);
	}
	walkMany(e, t, n) {
		this.checkIsTarget(e, R.REPETITION, t, n) || super.walkOption(e, t, n);
	}
	walkManySep(e, t, n) {
		this.checkIsTarget(e, R.REPETITION_WITH_SEPARATOR, t, n) || super.walkOption(e, t, n);
	}
}, pm = class extends Hd {
	constructor(e, t, n) {
		super(), this.targetOccurrence = e, this.targetProdType = t, this.targetRef = n, this.result = [];
	}
	checkIsTarget(e, t) {
		e.idx === this.targetOccurrence && this.targetProdType === t && (this.targetRef === void 0 || e === this.targetRef) && (this.result = e.definition);
	}
	visitOption(e) {
		this.checkIsTarget(e, R.OPTION);
	}
	visitRepetition(e) {
		this.checkIsTarget(e, R.REPETITION);
	}
	visitRepetitionMandatory(e) {
		this.checkIsTarget(e, R.REPETITION_MANDATORY);
	}
	visitRepetitionMandatoryWithSeparator(e) {
		this.checkIsTarget(e, R.REPETITION_MANDATORY_WITH_SEPARATOR);
	}
	visitRepetitionWithSeparator(e) {
		this.checkIsTarget(e, R.REPETITION_WITH_SEPARATOR);
	}
	visitAlternation(e) {
		this.checkIsTarget(e, R.ALTERNATION);
	}
};
function mm(e) {
	let t = Array(e);
	for (let n = 0; n < e; n++) t[n] = [];
	return t;
}
function hm(e) {
	let t = [""];
	for (let n = 0; n < e.length; n++) {
		let r = e[n], i = [];
		for (let e = 0; e < t.length; e++) {
			let n = t[e];
			i.push(n + "_" + r.tokenTypeIdx);
			for (let e = 0; e < r.categoryMatches.length; e++) {
				let t = "_" + r.categoryMatches[e];
				i.push(n + t);
			}
		}
		t = i;
	}
	return t;
}
function gm(e, t, n) {
	for (let r = 0; r < e.length; r++) {
		if (r === n) continue;
		let i = e[r];
		for (let e = 0; e < t.length; e++) if (i[t[e]] === !0) return !1;
	}
	return !0;
}
function _m(e, t) {
	let n = k(e, (e) => im([e], 1)), r = mm(n.length), i = k(n, (e) => {
		let t = {};
		return O(e, (e) => {
			O(hm(e.partialPath), (e) => {
				t[e] = !0;
			});
		}), t;
	}), a = n;
	for (let e = 1; e <= t; e++) {
		let n = a;
		a = mm(n.length);
		for (let o = 0; o < n.length; o++) {
			let s = n[o];
			for (let n = 0; n < s.length; n++) {
				let c = s[n].partialPath, l = s[n].suffixDef, u = hm(c);
				if (gm(i, u, o) || M(l) || c.length === t) {
					let e = r[o];
					if (bm(e, c) === !1) {
						e.push(c);
						for (let e = 0; e < u.length; e++) {
							let t = u[e];
							i[o][t] = !0;
						}
					}
				} else {
					let t = im(l, e + 1, c);
					a[o] = a[o].concat(t), O(t, (e) => {
						O(hm(e.partialPath), (e) => {
							i[o][e] = !0;
						});
					});
				}
			}
		}
	}
	return r;
}
function vm(e, t, n, r) {
	let i = new pm(e, R.ALTERNATION, r);
	return t.accept(i), _m(i.result, n);
}
function ym(e, t, n, r) {
	let i = new pm(e, n);
	t.accept(i);
	let a = i.result, o = new fm(t, e, n).startWalking();
	return _m([new Pd({ definition: a }), new Pd({ definition: o })], r);
}
function bm(e, t) {
	compareOtherPath: for (let n = 0; n < e.length; n++) {
		let r = e[n];
		if (r.length === t.length) {
			for (let e = 0; e < r.length; e++) {
				let n = t[e], i = r[e];
				if (n !== i && i.categoryMatchesMap[n.tokenTypeIdx] === void 0) continue compareOtherPath;
			}
			return !0;
		}
	}
	return !1;
}
function xm(e, t) {
	return e.length < t.length && cu(e, (e, n) => {
		let r = t[n];
		return e === r || r.categoryMatchesMap[e.tokenTypeIdx];
	});
}
function Sm(e) {
	return cu(e, (e) => cu(e, (e) => cu(e, (e) => M(e.categoryMatches))));
}
function Cm(e) {
	return k(e.lookaheadStrategy.validate({
		rules: e.rules,
		tokenTypes: e.tokenTypes,
		grammarName: e.grammarName
	}), (e) => Object.assign({ type: eg.CUSTOM_LOOKAHEAD_VALIDATION }, e));
}
function wm(e, t, n, r) {
	let i = wu(e, (e) => Tm(e, n)), a = Vm(e, t, n), o = wu(e, (e) => Lm(e, n)), s = wu(e, (t) => km(t, e, r, n));
	return i.concat(a, o, s);
}
function Tm(e, t) {
	let n = new Om();
	e.accept(n);
	let r = n.allProductions;
	return k(j(cd(Eu(r, Em), (e) => e.length > 1)), (n) => {
		let r = yu(n), i = t.buildDuplicateFoundError(e, n), a = Kd(r), o = {
			message: i,
			type: eg.DUPLICATE_PRODUCTIONS,
			ruleName: e.name,
			dslName: a,
			occurrence: r.idx
		}, s = Dm(r);
		return s && (o.parameter = s), o;
	});
}
function Em(e) {
	return `${Kd(e)}_#_${e.idx}_#_${Dm(e)}`;
}
function Dm(e) {
	return e instanceof P ? e.terminalType.name : e instanceof Md ? e.nonTerminalName : "";
}
var Om = class extends Hd {
	constructor() {
		super(...arguments), this.allProductions = [];
	}
	visitNonTerminal(e) {
		this.allProductions.push(e);
	}
	visitOption(e) {
		this.allProductions.push(e);
	}
	visitRepetitionWithSeparator(e) {
		this.allProductions.push(e);
	}
	visitRepetitionMandatory(e) {
		this.allProductions.push(e);
	}
	visitRepetitionMandatoryWithSeparator(e) {
		this.allProductions.push(e);
	}
	visitRepetition(e) {
		this.allProductions.push(e);
	}
	visitAlternation(e) {
		this.allProductions.push(e);
	}
	visitTerminal(e) {
		this.allProductions.push(e);
	}
};
function km(e, t, n, r) {
	let i = [];
	if (fd(t, (t, n) => n.name === e.name ? t + 1 : t, 0) > 1) {
		let t = r.buildDuplicateRuleNameError({
			topLevelRule: e,
			grammarName: n
		});
		i.push({
			message: t,
			type: eg.DUPLICATE_RULE_NAME,
			ruleName: e.name
		});
	}
	return i;
}
function Am(e, t, n) {
	let r = [], i;
	return zu(t, e) || (i = `Invalid rule override, rule: ->${e}<- cannot be overridden in the grammar: ->${n}<-as it is not defined in any of the super grammars `, r.push({
		message: i,
		type: eg.INVALID_RULE_OVERRIDE,
		ruleName: e
	})), r;
}
function jm(e, t, n, r = []) {
	let i = [], a = Mm(t.definition);
	if (M(a)) return [];
	{
		let t = e.name;
		zu(a, e) && i.push({
			message: n.buildLeftRecursionError({
				topLevelRule: e,
				leftRecursionPath: r
			}),
			type: eg.LEFT_RECURSION,
			ruleName: t
		});
		let o = wu(Jl(a, r.concat([e])), (t) => {
			let i = Qs(r);
			return i.push(t), jm(e, t, n, i);
		});
		return i.concat(o);
	}
}
function Mm(e) {
	let t = [];
	if (M(e)) return t;
	let n = yu(e);
	if (n instanceof Md) t.push(n.referencedRule);
	else if (n instanceof Pd || n instanceof Fd || n instanceof Id || n instanceof Ld || n instanceof Rd || n instanceof N) t = t.concat(Mm(n.definition));
	else if (n instanceof zd) t = Zi(k(n.definition, (e) => Mm(e.definition)));
	else if (!(n instanceof P)) throw Error("non exhaustive match");
	let r = Wd(n), i = e.length > 1;
	if (r && i) {
		let n = D(e);
		return t.concat(Mm(n));
	}
	return t;
}
var Nm = class extends Hd {
	constructor() {
		super(...arguments), this.alternations = [];
	}
	visitAlternation(e) {
		this.alternations.push(e);
	}
};
function Pm(e, t) {
	let n = new Nm();
	e.accept(n);
	let r = n.alternations;
	return wu(r, (n) => wu($l(n.definition), (r, i) => M(am([r], [], pp, 1)) ? [{
		message: t.buildEmptyAlternationError({
			topLevelRule: e,
			alternation: n,
			emptyChoiceIdx: i
		}),
		type: eg.NONE_LAST_EMPTY_ALT,
		ruleName: e.name,
		occurrence: n.idx,
		alternative: i + 1
	}] : []));
}
function Fm(e, t, n) {
	let r = new Nm();
	e.accept(r);
	let i = r.alternations;
	return i = md(i, (e) => e.ignoreAmbiguities === !0), wu(i, (r) => {
		let i = r.idx, a = vm(i, e, r.maxLookahead || t, r), o = zm(a, r, e, n), s = Bm(a, r, e, n);
		return o.concat(s);
	});
}
var Im = class extends Hd {
	constructor() {
		super(...arguments), this.allProductions = [];
	}
	visitRepetitionWithSeparator(e) {
		this.allProductions.push(e);
	}
	visitRepetitionMandatory(e) {
		this.allProductions.push(e);
	}
	visitRepetitionMandatoryWithSeparator(e) {
		this.allProductions.push(e);
	}
	visitRepetition(e) {
		this.allProductions.push(e);
	}
};
function Lm(e, t) {
	let n = new Nm();
	e.accept(n);
	let r = n.alternations;
	return wu(r, (n) => n.definition.length > 255 ? [{
		message: t.buildTooManyAlternativesError({
			topLevelRule: e,
			alternation: n
		}),
		type: eg.TOO_MANY_ALTS,
		ruleName: e.name,
		occurrence: n.idx
	}] : []);
}
function Rm(e, t, n) {
	let r = [];
	return O(e, (e) => {
		let i = new Im();
		e.accept(i);
		let a = i.allProductions;
		O(a, (i) => {
			let a = sm(i), o = i.maxLookahead || t, s = i.idx, c = ym(s, e, a, o)[0];
			if (M(Zi(c))) {
				let t = n.buildEmptyRepetitionError({
					topLevelRule: e,
					repetition: i
				});
				r.push({
					message: t,
					type: eg.NO_NON_EMPTY_LOOKAHEAD,
					ruleName: e.name
				});
			}
		});
	}), r;
}
function zm(e, t, n, r) {
	let i = [];
	return k(fd(e, (n, r, a) => (t.definition[a].ignoreAmbiguities === !0 || O(r, (r) => {
		let o = [a];
		O(e, (e, n) => {
			a !== n && bm(e, r) && t.definition[n].ignoreAmbiguities !== !0 && o.push(n);
		}), o.length > 1 && !bm(i, r) && (i.push(r), n.push({
			alts: o,
			path: r
		}));
	}), n), []), (e) => {
		let i = k(e.alts, (e) => e + 1);
		return {
			message: r.buildAlternationAmbiguityError({
				topLevelRule: n,
				alternation: t,
				ambiguityIndices: i,
				prefixPath: e.path
			}),
			type: eg.AMBIGUOUS_ALTS,
			ruleName: n.name,
			occurrence: t.idx,
			alternatives: e.alts
		};
	});
}
function Bm(e, t, n, r) {
	let i = fd(e, (e, t, n) => {
		let r = k(t, (e) => ({
			idx: n,
			path: e
		}));
		return e.concat(r);
	}, []);
	return ec(wu(i, (e) => {
		if (t.definition[e.idx].ignoreAmbiguities === !0) return [];
		let a = e.idx, o = e.path;
		return k(fu(i, (e) => t.definition[e.idx].ignoreAmbiguities !== !0 && e.idx < a && xm(e.path, o)), (e) => {
			let i = [e.idx + 1, a + 1], o = t.idx === 0 ? "" : t.idx;
			return {
				message: r.buildAlternationPrefixAmbiguityError({
					topLevelRule: n,
					alternation: t,
					ambiguityIndices: i,
					prefixPath: e.path
				}),
				type: eg.AMBIGUOUS_PREFIX_ALTS,
				ruleName: n.name,
				occurrence: o,
				alternatives: i
			};
		});
	}));
}
function Vm(e, t, n) {
	let r = [], i = k(t, (e) => e.name);
	return O(e, (e) => {
		let t = e.name;
		if (zu(i, t)) {
			let i = n.buildNamespaceConflictError(e);
			r.push({
				message: i,
				type: eg.CONFLICT_TOKENS_RULES_NAMESPACE,
				ruleName: t
			});
		}
	}), r;
}
function Hm(e) {
	let t = Bl(e, { errMsgProvider: qp }), n = {};
	return O(e.rules, (e) => {
		n[e.name] = e;
	}), Yp(n, t.errMsgProvider);
}
function Um(e) {
	return e = Bl(e, { errMsgProvider: Jp }), wm(e.rules, e.tokenTypes, e.errMsgProvider, e.grammarName);
}
var Wm = "MismatchedTokenException", Gm = "NoViableAltException", Km = "EarlyExitException", qm = "NotAllInputParsedException", Jm = [
	Wm,
	Gm,
	Km,
	qm
];
Object.freeze(Jm);
function Ym(e) {
	return zu(Jm, e.name);
}
var Xm = class extends Error {
	constructor(e, t) {
		super(e), this.token = t, this.resyncedTokens = [], Object.setPrototypeOf(this, new.target.prototype), Error.captureStackTrace && Error.captureStackTrace(this, this.constructor);
	}
}, Zm = class extends Xm {
	constructor(e, t, n) {
		super(e, t), this.previousToken = n, this.name = Wm;
	}
}, Qm = class extends Xm {
	constructor(e, t, n) {
		super(e, t), this.previousToken = n, this.name = Gm;
	}
}, $m = class extends Xm {
	constructor(e, t) {
		super(e, t), this.name = qm;
	}
}, eh = class extends Xm {
	constructor(e, t, n) {
		super(e, t), this.previousToken = n, this.name = Km;
	}
}, th = {}, nh = "InRuleRecoveryException", rh = class extends Error {
	constructor(e) {
		super(e), this.name = nh;
	}
}, ih = class {
	initRecoverable(e) {
		this.firstAfterRepMap = {}, this.resyncFollows = {}, this.recoveryEnabled = A(e, "recoveryEnabled") ? e.recoveryEnabled : Qh.recoveryEnabled, this.recoveryEnabled && (this.attemptInRepetitionRecovery = ah);
	}
	getTokenToInsert(e) {
		let t = Wp(e, "", NaN, NaN, NaN, NaN, NaN, NaN);
		return t.isInsertedInRecovery = !0, t;
	}
	canTokenTypeBeInsertedInRecovery(e) {
		return !0;
	}
	canTokenTypeBeDeletedInRecovery(e) {
		return !0;
	}
	tryInRepetitionRecovery(e, t, n, r) {
		let i = this.findReSyncTokenType(), a = this.exportLexerState(), o = [], s = !1, c = this.LA_FAST(1), l = this.LA_FAST(1), u = () => {
			let e = this.LA(0), t = new Zm(this.errorMessageProvider.buildMismatchTokenMessage({
				expected: r,
				actual: c,
				previous: e,
				ruleName: this.getCurrRuleFullName()
			}), c, this.LA(0));
			t.resyncedTokens = $l(o), this.SAVE_ERROR(t);
		};
		for (; !s;) if (this.tokenMatcher(l, r)) {
			u();
			return;
		} else if (n.call(this)) {
			u(), e.apply(this, t);
			return;
		} else this.tokenMatcher(l, i) ? s = !0 : (l = this.SKIP_TOKEN(), this.addToResyncTokens(l, o));
		this.importLexerState(a);
	}
	shouldInRepetitionRecoveryBeTried(e, t, n) {
		return !(n === !1 || this.tokenMatcher(this.LA_FAST(1), e) || this.isBackTracking() || this.canPerformInRuleRecovery(e, this.getFollowsForInRuleRecovery(e, t)));
	}
	getFollowsForInRuleRecovery(e, t) {
		let n = this.getCurrentGrammarPath(e, t);
		return this.getNextPossibleTokenTypes(n);
	}
	tryInRuleRecovery(e, t) {
		if (this.canRecoverWithSingleTokenInsertion(e, t)) return this.getTokenToInsert(e);
		if (this.canRecoverWithSingleTokenDeletion(e)) {
			let e = this.SKIP_TOKEN();
			return this.consumeToken(), e;
		}
		throw new rh("sad sad panda");
	}
	canPerformInRuleRecovery(e, t) {
		return this.canRecoverWithSingleTokenInsertion(e, t) || this.canRecoverWithSingleTokenDeletion(e);
	}
	canRecoverWithSingleTokenInsertion(e, t) {
		if (!this.canTokenTypeBeInsertedInRecovery(e) || M(t)) return !1;
		let n = this.LA_FAST(1);
		return _u(t, (e) => this.tokenMatcher(n, e)) !== void 0;
	}
	canRecoverWithSingleTokenDeletion(e) {
		return this.canTokenTypeBeDeletedInRecovery(e) ? this.tokenMatcher(this.LA(2), e) : !1;
	}
	isInCurrentRuleReSyncSet(e) {
		let t = this.getCurrFollowKey();
		return zu(this.getFollowSetFromFollowKey(t), e);
	}
	findReSyncTokenType() {
		let e = this.flattenFollowSet(), t = this.LA_FAST(1), n = 2;
		for (;;) {
			let r = _u(e, (e) => Gp(t, e));
			if (r !== void 0) return r;
			t = this.LA(n), n++;
		}
	}
	getCurrFollowKey() {
		if (this.RULE_STACK_IDX === 0) return th;
		let e = this.currRuleShortName, t = this.getLastExplicitRuleOccurrenceIndex(), n = this.getPreviousExplicitRuleShortName();
		return {
			ruleName: this.shortRuleNameToFullName(e),
			idxInCallingRule: t,
			inRule: this.shortRuleNameToFullName(n)
		};
	}
	buildFullFollowKeyStack() {
		let e = this.RULE_STACK, t = this.RULE_OCCURRENCE_STACK, n = this.RULE_STACK_IDX + 1, r = Array(n);
		for (let i = 0; i < n; i++) i === 0 ? r[i] = th : r[i] = {
			ruleName: this.shortRuleNameToFullName(e[i]),
			idxInCallingRule: t[i],
			inRule: this.shortRuleNameToFullName(e[i - 1])
		};
		return r;
	}
	flattenFollowSet() {
		return Zi(k(this.buildFullFollowKeyStack(), (e) => this.getFollowSetFromFollowKey(e)));
	}
	getFollowSetFromFollowKey(e) {
		if (e === th) return [Up];
		let t = e.ruleName + e.idxInCallingRule + $d + e.inRule;
		return this.resyncFollows[t];
	}
	addToResyncTokens(e, t) {
		return this.tokenMatcher(e, Up) || t.push(e), t;
	}
	reSyncTo(e) {
		let t = [], n = this.LA_FAST(1);
		for (; this.tokenMatcher(n, e) === !1;) n = this.SKIP_TOKEN(), this.addToResyncTokens(n, t);
		return $l(t);
	}
	attemptInRepetitionRecovery(e, t, n, r, i, a, o) {}
	getCurrentGrammarPath(e, t) {
		return {
			ruleStack: this.getHumanReadableRuleStack(),
			occurrenceStack: this.RULE_OCCURRENCE_STACK.slice(0, this.RULE_OCCURRENCE_STACK_IDX + 1),
			lastTok: e,
			lastTokOccurrence: t
		};
	}
	getHumanReadableRuleStack() {
		let e = this.RULE_STACK_IDX + 1, t = Array(e);
		for (let n = 0; n < e; n++) t[n] = this.shortRuleNameToFullName(this.RULE_STACK[n]);
		return t;
	}
};
function ah(e, t, n, r, i, a, o) {
	let s = this.getKeyForAutomaticLookahead(r, i), c = this.firstAfterRepMap[s];
	if (c === void 0) {
		let e = this.getCurrRuleFullName(), t = this.getGAstProductions()[e];
		c = new a(t, i).startWalking(), this.firstAfterRepMap[s] = c;
	}
	let l = c.token, u = c.occurrence, d = c.isEndOfRule;
	this.RULE_STACK_IDX === 0 && d && l === void 0 && (l = Up, u = 1), l !== void 0 && u !== void 0 && this.shouldInRepetitionRecoveryBeTried(l, u, o) && this.tryInRepetitionRecovery(e, t, n, l);
}
var oh = 4, sh = 8, ch = 8, lh = 1 << sh, uh = 2 << sh, dh = 3 << sh, fh = 4 << sh, ph = 5 << sh, mh = 6 << sh;
function hh(e, t, n) {
	return n | t | e;
}
32 - ch;
var gh = class {
	constructor(e) {
		this.maxLookahead = e?.maxLookahead ?? Qh.maxLookahead;
	}
	validate(e) {
		let t = this.validateNoLeftRecursion(e.rules);
		if (M(t)) {
			let n = this.validateEmptyOrAlternatives(e.rules), r = this.validateAmbiguousAlternationAlternatives(e.rules, this.maxLookahead), i = this.validateSomeNonEmptyLookaheadPath(e.rules, this.maxLookahead);
			return [
				...t,
				...n,
				...r,
				...i
			];
		}
		return t;
	}
	validateNoLeftRecursion(e) {
		return wu(e, (e) => jm(e, e, Jp));
	}
	validateEmptyOrAlternatives(e) {
		return wu(e, (e) => Pm(e, Jp));
	}
	validateAmbiguousAlternationAlternatives(e, t) {
		return wu(e, (e) => Fm(e, t, Jp));
	}
	validateSomeNonEmptyLookaheadPath(e, t) {
		return Rm(e, t, Jp);
	}
	buildLookaheadForAlternation(e) {
		return cm(e.prodOccurrence, e.rule, e.maxLookahead, e.hasPredicates, e.dynamicTokensEnabled, um);
	}
	buildLookaheadForOptional(e) {
		return lm(e.prodOccurrence, e.rule, e.maxLookahead, e.dynamicTokensEnabled, sm(e.prodType), dm);
	}
}, _h = class {
	initLooksAhead(e) {
		this.dynamicTokensEnabled = A(e, "dynamicTokensEnabled") ? e.dynamicTokensEnabled : Qh.dynamicTokensEnabled, this.maxLookahead = A(e, "maxLookahead") ? e.maxLookahead : Qh.maxLookahead, this.lookaheadStrategy = A(e, "lookaheadStrategy") ? e.lookaheadStrategy : new gh({ maxLookahead: this.maxLookahead }), this.lookAheadFuncsCache = /* @__PURE__ */ new Map();
	}
	preComputeLookaheadFunctions(e) {
		O(e, (e) => {
			this.TRACE_INIT(`${e.name} Rule Lookahead`, () => {
				let { alternation: t, repetition: n, option: r, repetitionMandatory: i, repetitionMandatoryWithSeparator: a, repetitionWithSeparator: o } = yh(e);
				O(t, (t) => {
					let n = t.idx === 0 ? "" : t.idx;
					this.TRACE_INIT(`${Kd(t)}${n}`, () => {
						let n = this.lookaheadStrategy.buildLookaheadForAlternation({
							prodOccurrence: t.idx,
							rule: e,
							maxLookahead: t.maxLookahead || this.maxLookahead,
							hasPredicates: t.hasPredicates,
							dynamicTokensEnabled: this.dynamicTokensEnabled
						}), r = hh(this.fullRuleNameToShort[e.name], lh, t.idx);
						this.setLaFuncCache(r, n);
					});
				}), O(n, (t) => {
					this.computeLookaheadFunc(e, t.idx, dh, "Repetition", t.maxLookahead, Kd(t));
				}), O(r, (t) => {
					this.computeLookaheadFunc(e, t.idx, uh, "Option", t.maxLookahead, Kd(t));
				}), O(i, (t) => {
					this.computeLookaheadFunc(e, t.idx, fh, "RepetitionMandatory", t.maxLookahead, Kd(t));
				}), O(a, (t) => {
					this.computeLookaheadFunc(e, t.idx, mh, "RepetitionMandatoryWithSeparator", t.maxLookahead, Kd(t));
				}), O(o, (t) => {
					this.computeLookaheadFunc(e, t.idx, ph, "RepetitionWithSeparator", t.maxLookahead, Kd(t));
				});
			});
		});
	}
	computeLookaheadFunc(e, t, n, r, i, a) {
		this.TRACE_INIT(`${a}${t === 0 ? "" : t}`, () => {
			let a = this.lookaheadStrategy.buildLookaheadForOptional({
				prodOccurrence: t,
				rule: e,
				maxLookahead: i || this.maxLookahead,
				dynamicTokensEnabled: this.dynamicTokensEnabled,
				prodType: r
			}), o = hh(this.fullRuleNameToShort[e.name], n, t);
			this.setLaFuncCache(o, a);
		});
	}
	getKeyForAutomaticLookahead(e, t) {
		return hh(this.currRuleShortName, e, t);
	}
	getLaFuncFromCache(e) {
		return this.lookAheadFuncsCache.get(e);
	}
	/* istanbul ignore next */
	setLaFuncCache(e, t) {
		this.lookAheadFuncsCache.set(e, t);
	}
}, vh = new class extends Hd {
	constructor() {
		super(...arguments), this.dslMethods = {
			option: [],
			alternation: [],
			repetition: [],
			repetitionWithSeparator: [],
			repetitionMandatory: [],
			repetitionMandatoryWithSeparator: []
		};
	}
	reset() {
		this.dslMethods = {
			option: [],
			alternation: [],
			repetition: [],
			repetitionWithSeparator: [],
			repetitionMandatory: [],
			repetitionMandatoryWithSeparator: []
		};
	}
	visitOption(e) {
		this.dslMethods.option.push(e);
	}
	visitRepetitionWithSeparator(e) {
		this.dslMethods.repetitionWithSeparator.push(e);
	}
	visitRepetitionMandatory(e) {
		this.dslMethods.repetitionMandatory.push(e);
	}
	visitRepetitionMandatoryWithSeparator(e) {
		this.dslMethods.repetitionMandatoryWithSeparator.push(e);
	}
	visitRepetition(e) {
		this.dslMethods.repetition.push(e);
	}
	visitAlternation(e) {
		this.dslMethods.alternation.push(e);
	}
}();
function yh(e) {
	vh.reset(), e.accept(vh);
	let t = vh.dslMethods;
	return vh.reset(), t;
}
function bh(e, t) {
	isNaN(e.startOffset) === !0 ? (e.startOffset = t.startOffset, e.endOffset = t.endOffset) : e.endOffset < t.endOffset && (e.endOffset = t.endOffset);
}
function xh(e, t) {
	isNaN(e.startOffset) === !0 ? (e.startOffset = t.startOffset, e.startColumn = t.startColumn, e.startLine = t.startLine, e.endOffset = t.endOffset, e.endColumn = t.endColumn, e.endLine = t.endLine) : e.endOffset < t.endOffset && (e.endOffset = t.endOffset, e.endColumn = t.endColumn, e.endLine = t.endLine);
}
function Sh(e, t, n) {
	e.children[n] === void 0 ? e.children[n] = [t] : e.children[n].push(t);
}
function Ch(e, t, n) {
	e.children[t] === void 0 ? e.children[t] = [n] : e.children[t].push(n);
}
var wh = "name";
function Th(e, t) {
	Object.defineProperty(e, wh, {
		enumerable: !1,
		configurable: !0,
		writable: !1,
		value: t
	});
}
function Eh(e, t) {
	let n = mr(e), r = n.length;
	for (let i = 0; i < r; i++) {
		let r = e[n[i]], a = r.length;
		for (let e = 0; e < a; e++) {
			let n = r[e];
			n.tokenTypeIdx === void 0 && this[n.name](n.children, t);
		}
	}
}
function Dh(e, t) {
	let n = function() {};
	return Th(n, e + "BaseSemantics"), n.prototype = {
		visit: function(e, t) {
			if (C(e) && (e = e[0]), !$u(e)) return this[e.name](e.children, t);
		},
		validateVisitor: function() {
			let e = Ah(this, t);
			if (!M(e)) {
				let t = k(e, (e) => e.msg);
				throw Error(`Errors Detected in CST Visitor <${this.constructor.name}>:
	${t.join("\n\n").replace(/\n/g, "\n	")}`);
			}
		}
	}, n.prototype.constructor = n, n._RULE_NAMES = t, n;
}
function Oh(e, t, n) {
	let r = function() {};
	Th(r, e + "BaseSemanticsWithDefaults");
	let i = Object.create(n.prototype);
	return O(t, (e) => {
		i[e] = Eh;
	}), r.prototype = i, r.prototype.constructor = r, r;
}
var kh;
(function(e) {
	e[e.REDUNDANT_METHOD = 0] = "REDUNDANT_METHOD", e[e.MISSING_METHOD = 1] = "MISSING_METHOD";
})(kh ||= {});
function Ah(e, t) {
	return jh(e, t);
}
function jh(e, t) {
	return ec(k(fu(t, (t) => Ie(e[t]) === !1), (t) => ({
		msg: `Missing visitor method: <${t}> on ${e.constructor.name} CST Visitor.`,
		type: kh.MISSING_METHOD,
		methodName: t
	})));
}
var Mh = class {
	initTreeBuilder(e) {
		if (this.CST_STACK = [], this.outputCst = e.outputCst, this.nodeLocationTracking = A(e, "nodeLocationTracking") ? e.nodeLocationTracking : Qh.nodeLocationTracking, !this.outputCst) this.cstInvocationStateUpdate = w, this.cstFinallyStateUpdate = w, this.cstPostTerminal = w, this.cstPostNonTerminal = w, this.cstPostRule = w;
		else if (/full/i.test(this.nodeLocationTracking)) this.recoveryEnabled ? (this.setNodeLocationFromToken = xh, this.setNodeLocationFromNode = xh, this.cstPostRule = w, this.setInitialNodeLocation = this.setInitialNodeLocationFullRecovery) : (this.setNodeLocationFromToken = w, this.setNodeLocationFromNode = w, this.cstPostRule = this.cstPostRuleFull, this.setInitialNodeLocation = this.setInitialNodeLocationFullRegular);
		else if (/onlyOffset/i.test(this.nodeLocationTracking)) this.recoveryEnabled ? (this.setNodeLocationFromToken = bh, this.setNodeLocationFromNode = bh, this.cstPostRule = w, this.setInitialNodeLocation = this.setInitialNodeLocationOnlyOffsetRecovery) : (this.setNodeLocationFromToken = w, this.setNodeLocationFromNode = w, this.cstPostRule = this.cstPostRuleOnlyOffset, this.setInitialNodeLocation = this.setInitialNodeLocationOnlyOffsetRegular);
		else if (/none/i.test(this.nodeLocationTracking)) this.setNodeLocationFromToken = w, this.setNodeLocationFromNode = w, this.cstPostRule = w, this.setInitialNodeLocation = w;
		else throw Error(`Invalid <nodeLocationTracking> config option: "${e.nodeLocationTracking}"`);
	}
	setInitialNodeLocationOnlyOffsetRecovery(e) {
		e.location = {
			startOffset: NaN,
			endOffset: NaN
		};
	}
	setInitialNodeLocationOnlyOffsetRegular(e) {
		e.location = {
			startOffset: this.LA_FAST(1).startOffset,
			endOffset: NaN
		};
	}
	setInitialNodeLocationFullRecovery(e) {
		e.location = {
			startOffset: NaN,
			startLine: NaN,
			startColumn: NaN,
			endOffset: NaN,
			endLine: NaN,
			endColumn: NaN
		};
	}
	setInitialNodeLocationFullRegular(e) {
		let t = this.LA_FAST(1);
		e.location = {
			startOffset: t.startOffset,
			startLine: t.startLine,
			startColumn: t.startColumn,
			endOffset: NaN,
			endLine: NaN,
			endColumn: NaN
		};
	}
	cstInvocationStateUpdate(e) {
		let t = {
			name: e,
			children: /* @__PURE__ */ Object.create(null)
		};
		this.setInitialNodeLocation(t), this.CST_STACK.push(t);
	}
	cstFinallyStateUpdate() {
		this.CST_STACK.pop();
	}
	cstPostRuleFull(e) {
		let t = this.LA(0), n = e.location;
		n.startOffset <= t.startOffset ? (n.endOffset = t.endOffset, n.endLine = t.endLine, n.endColumn = t.endColumn) : (n.startOffset = NaN, n.startLine = NaN, n.startColumn = NaN);
	}
	cstPostRuleOnlyOffset(e) {
		let t = this.LA(0), n = e.location;
		n.startOffset <= t.startOffset ? n.endOffset = t.endOffset : n.startOffset = NaN;
	}
	cstPostTerminal(e, t) {
		let n = this.CST_STACK[this.CST_STACK.length - 1];
		Sh(n, t, e), this.setNodeLocationFromToken(n.location, t);
	}
	cstPostNonTerminal(e, t) {
		let n = this.CST_STACK[this.CST_STACK.length - 1];
		Ch(n, t, e), this.setNodeLocationFromNode(n.location, e.location);
	}
	getBaseCstVisitorConstructor() {
		if ($u(this.baseCstVisitorConstructor)) {
			let e = Dh(this.className, mr(this.gastProductionsCache));
			return this.baseCstVisitorConstructor = e, e;
		}
		return this.baseCstVisitorConstructor;
	}
	getBaseCstVisitorConstructorWithDefaults() {
		if ($u(this.baseCstVisitorWithDefaultsConstructor)) {
			let e = Oh(this.className, mr(this.gastProductionsCache), this.getBaseCstVisitorConstructor());
			return this.baseCstVisitorWithDefaultsConstructor = e, e;
		}
		return this.baseCstVisitorWithDefaultsConstructor;
	}
	getPreviousExplicitRuleShortName() {
		return this.RULE_STACK[this.RULE_STACK_IDX - 1];
	}
	getLastExplicitRuleOccurrenceIndex() {
		return this.RULE_OCCURRENCE_STACK[this.RULE_OCCURRENCE_STACK_IDX];
	}
}, Nh = class {
	initLexerAdapter() {
		this.tokVector = [], this.tokVectorLength = 0, this.currIdx = -1;
	}
	set input(e) {
		if (this.selfAnalysisDone !== !0) throw Error("Missing <performSelfAnalysis> invocation at the end of the Parser's constructor.");
		this.reset(), this.tokVector = e, this.tokVectorLength = e.length;
	}
	get input() {
		return this.tokVector;
	}
	SKIP_TOKEN() {
		return this.currIdx <= this.tokVectorLength - 2 ? (this.consumeToken(), this.LA_FAST(1)) : Zh;
	}
	LA_FAST(e) {
		let t = this.currIdx + e;
		return this.tokVector[t];
	}
	LA(e) {
		let t = this.currIdx + e;
		return t < 0 || this.tokVectorLength <= t ? Zh : this.tokVector[t];
	}
	consumeToken() {
		this.currIdx++;
	}
	exportLexerState() {
		return this.currIdx;
	}
	importLexerState(e) {
		this.currIdx = e;
	}
	resetLexerState() {
		this.currIdx = -1;
	}
	moveToTerminatedState() {
		this.currIdx = this.tokVectorLength - 1;
	}
	getLexerPosition() {
		return this.exportLexerState();
	}
}, Ph = class {
	ACTION(e) {
		return e.call(this);
	}
	consume(e, t, n) {
		return this.consumeInternal(t, e, n);
	}
	subrule(e, t, n) {
		return this.subruleInternal(t, e, n);
	}
	option(e, t) {
		return this.optionInternal(t, e);
	}
	or(e, t) {
		return this.orInternal(t, e);
	}
	many(e, t) {
		return this.manyInternal(e, t);
	}
	atLeastOne(e, t) {
		return this.atLeastOneInternal(e, t);
	}
	CONSUME(e, t) {
		return this.consumeInternal(e, 0, t);
	}
	CONSUME1(e, t) {
		return this.consumeInternal(e, 1, t);
	}
	CONSUME2(e, t) {
		return this.consumeInternal(e, 2, t);
	}
	CONSUME3(e, t) {
		return this.consumeInternal(e, 3, t);
	}
	CONSUME4(e, t) {
		return this.consumeInternal(e, 4, t);
	}
	CONSUME5(e, t) {
		return this.consumeInternal(e, 5, t);
	}
	CONSUME6(e, t) {
		return this.consumeInternal(e, 6, t);
	}
	CONSUME7(e, t) {
		return this.consumeInternal(e, 7, t);
	}
	CONSUME8(e, t) {
		return this.consumeInternal(e, 8, t);
	}
	CONSUME9(e, t) {
		return this.consumeInternal(e, 9, t);
	}
	SUBRULE(e, t) {
		return this.subruleInternal(e, 0, t);
	}
	SUBRULE1(e, t) {
		return this.subruleInternal(e, 1, t);
	}
	SUBRULE2(e, t) {
		return this.subruleInternal(e, 2, t);
	}
	SUBRULE3(e, t) {
		return this.subruleInternal(e, 3, t);
	}
	SUBRULE4(e, t) {
		return this.subruleInternal(e, 4, t);
	}
	SUBRULE5(e, t) {
		return this.subruleInternal(e, 5, t);
	}
	SUBRULE6(e, t) {
		return this.subruleInternal(e, 6, t);
	}
	SUBRULE7(e, t) {
		return this.subruleInternal(e, 7, t);
	}
	SUBRULE8(e, t) {
		return this.subruleInternal(e, 8, t);
	}
	SUBRULE9(e, t) {
		return this.subruleInternal(e, 9, t);
	}
	OPTION(e) {
		return this.optionInternal(e, 0);
	}
	OPTION1(e) {
		return this.optionInternal(e, 1);
	}
	OPTION2(e) {
		return this.optionInternal(e, 2);
	}
	OPTION3(e) {
		return this.optionInternal(e, 3);
	}
	OPTION4(e) {
		return this.optionInternal(e, 4);
	}
	OPTION5(e) {
		return this.optionInternal(e, 5);
	}
	OPTION6(e) {
		return this.optionInternal(e, 6);
	}
	OPTION7(e) {
		return this.optionInternal(e, 7);
	}
	OPTION8(e) {
		return this.optionInternal(e, 8);
	}
	OPTION9(e) {
		return this.optionInternal(e, 9);
	}
	OR(e) {
		return this.orInternal(e, 0);
	}
	OR1(e) {
		return this.orInternal(e, 1);
	}
	OR2(e) {
		return this.orInternal(e, 2);
	}
	OR3(e) {
		return this.orInternal(e, 3);
	}
	OR4(e) {
		return this.orInternal(e, 4);
	}
	OR5(e) {
		return this.orInternal(e, 5);
	}
	OR6(e) {
		return this.orInternal(e, 6);
	}
	OR7(e) {
		return this.orInternal(e, 7);
	}
	OR8(e) {
		return this.orInternal(e, 8);
	}
	OR9(e) {
		return this.orInternal(e, 9);
	}
	MANY(e) {
		this.manyInternal(0, e);
	}
	MANY1(e) {
		this.manyInternal(1, e);
	}
	MANY2(e) {
		this.manyInternal(2, e);
	}
	MANY3(e) {
		this.manyInternal(3, e);
	}
	MANY4(e) {
		this.manyInternal(4, e);
	}
	MANY5(e) {
		this.manyInternal(5, e);
	}
	MANY6(e) {
		this.manyInternal(6, e);
	}
	MANY7(e) {
		this.manyInternal(7, e);
	}
	MANY8(e) {
		this.manyInternal(8, e);
	}
	MANY9(e) {
		this.manyInternal(9, e);
	}
	MANY_SEP(e) {
		this.manySepFirstInternal(0, e);
	}
	MANY_SEP1(e) {
		this.manySepFirstInternal(1, e);
	}
	MANY_SEP2(e) {
		this.manySepFirstInternal(2, e);
	}
	MANY_SEP3(e) {
		this.manySepFirstInternal(3, e);
	}
	MANY_SEP4(e) {
		this.manySepFirstInternal(4, e);
	}
	MANY_SEP5(e) {
		this.manySepFirstInternal(5, e);
	}
	MANY_SEP6(e) {
		this.manySepFirstInternal(6, e);
	}
	MANY_SEP7(e) {
		this.manySepFirstInternal(7, e);
	}
	MANY_SEP8(e) {
		this.manySepFirstInternal(8, e);
	}
	MANY_SEP9(e) {
		this.manySepFirstInternal(9, e);
	}
	AT_LEAST_ONE(e) {
		this.atLeastOneInternal(0, e);
	}
	AT_LEAST_ONE1(e) {
		return this.atLeastOneInternal(1, e);
	}
	AT_LEAST_ONE2(e) {
		this.atLeastOneInternal(2, e);
	}
	AT_LEAST_ONE3(e) {
		this.atLeastOneInternal(3, e);
	}
	AT_LEAST_ONE4(e) {
		this.atLeastOneInternal(4, e);
	}
	AT_LEAST_ONE5(e) {
		this.atLeastOneInternal(5, e);
	}
	AT_LEAST_ONE6(e) {
		this.atLeastOneInternal(6, e);
	}
	AT_LEAST_ONE7(e) {
		this.atLeastOneInternal(7, e);
	}
	AT_LEAST_ONE8(e) {
		this.atLeastOneInternal(8, e);
	}
	AT_LEAST_ONE9(e) {
		this.atLeastOneInternal(9, e);
	}
	AT_LEAST_ONE_SEP(e) {
		this.atLeastOneSepFirstInternal(0, e);
	}
	AT_LEAST_ONE_SEP1(e) {
		this.atLeastOneSepFirstInternal(1, e);
	}
	AT_LEAST_ONE_SEP2(e) {
		this.atLeastOneSepFirstInternal(2, e);
	}
	AT_LEAST_ONE_SEP3(e) {
		this.atLeastOneSepFirstInternal(3, e);
	}
	AT_LEAST_ONE_SEP4(e) {
		this.atLeastOneSepFirstInternal(4, e);
	}
	AT_LEAST_ONE_SEP5(e) {
		this.atLeastOneSepFirstInternal(5, e);
	}
	AT_LEAST_ONE_SEP6(e) {
		this.atLeastOneSepFirstInternal(6, e);
	}
	AT_LEAST_ONE_SEP7(e) {
		this.atLeastOneSepFirstInternal(7, e);
	}
	AT_LEAST_ONE_SEP8(e) {
		this.atLeastOneSepFirstInternal(8, e);
	}
	AT_LEAST_ONE_SEP9(e) {
		this.atLeastOneSepFirstInternal(9, e);
	}
	RULE(e, t, n = $h) {
		if (zu(this.definedRulesNames, e)) {
			let t = {
				message: Jp.buildDuplicateRuleNameError({
					topLevelRule: e,
					grammarName: this.className
				}),
				type: eg.DUPLICATE_RULE_NAME,
				ruleName: e
			};
			this.definitionErrors.push(t);
		}
		this.definedRulesNames.push(e);
		let r = this.defineRule(e, t, n);
		return this[e] = r, r;
	}
	OVERRIDE_RULE(e, t, n = $h) {
		let r = Am(e, this.definedRulesNames, this.className);
		this.definitionErrors = this.definitionErrors.concat(r);
		let i = this.defineRule(e, t, n);
		return this[e] = i, i;
	}
	BACKTRACK(e, t) {
		let n = e.coreRule ?? e;
		return function() {
			this.isBackTrackingStack.push(1);
			let e = this.saveRecogState();
			try {
				return n.apply(this, t), !0;
			} catch (e) {
				if (Ym(e)) return !1;
				throw e;
			} finally {
				this.reloadRecogState(e), this.isBackTrackingStack.pop();
			}
		};
	}
	getGAstProductions() {
		return this.gastProductionsCache;
	}
	getSerializedGastProductions() {
		return Bd(j(this.gastProductionsCache));
	}
}, Fh = class {
	initRecognizerEngine(e, t) {
		if (this.className = this.constructor.name, this.shortRuleNameToFull = {}, this.fullRuleNameToShort = {}, this.ruleShortNameIdx = 256, this.tokenMatcher = mp, this.subruleIdx = 0, this.currRuleShortName = 0, this.definedRulesNames = [], this.tokensMap = {}, this.isBackTrackingStack = [], this.RULE_STACK = [], this.RULE_STACK_IDX = -1, this.RULE_OCCURRENCE_STACK = [], this.RULE_OCCURRENCE_STACK_IDX = -1, this.gastProductionsCache = {}, A(t, "serializedGrammar")) throw Error("The Parser's configuration can no longer contain a <serializedGrammar> property.\n	See: https://chevrotain.io/docs/changes/BREAKING_CHANGES.html#_6-0-0\n	For Further details.");
		if (C(e)) {
			if (M(e)) throw Error("A Token Vocabulary cannot be empty.\n	Note that the first argument for the parser constructor\n	is no longer a Token vector (since v4.0).");
			if (typeof e[0].startOffset == "number") throw Error("The Parser constructor no longer accepts a token vector as the first argument.\n	See: https://chevrotain.io/docs/changes/BREAKING_CHANGES.html#_4-0-0\n	For Further details.");
		}
		if (C(e)) this.tokensMap = fd(e, (e, t) => (e[t.name] = t, e), {});
		else if (A(e, "modes") && cu(Zi(j(e.modes)), Dp)) {
			let t = wd(Zi(j(e.modes)));
			this.tokensMap = fd(t, (e, t) => (e[t.name] = t, e), {});
		} else if (he(e)) this.tokensMap = Qs(e);
		else throw Error("<tokensDictionary> argument must be An Array of Token constructors, A dictionary of Token constructors or an IMultiModeLexerDefinition");
		this.tokensMap.EOF = Up;
		let n = cu(A(e, "modes") ? Zi(j(e.modes)) : j(e), (e) => M(e.categoryMatches));
		this.tokenMatcher = n ? mp : pp, _p(j(this.tokensMap));
	}
	defineRule(e, t, n) {
		if (this.selfAnalysisDone) throw Error(`Grammar rule <${e}> may not be defined after the 'performSelfAnalysis' method has been called'
Make sure that all grammar rule definitions are done before 'performSelfAnalysis' is called.`);
		let r = A(n, "resyncEnabled") ? n.resyncEnabled : $h.resyncEnabled, i = A(n, "recoveryValueFunc") ? n.recoveryValueFunc : $h.recoveryValueFunc, a = this.ruleShortNameIdx << oh + sh;
		this.ruleShortNameIdx++, this.shortRuleNameToFull[a] = e, this.fullRuleNameToShort[e] = a;
		let o;
		return o = this.outputCst === !0 ? function(...n) {
			try {
				this.ruleInvocationStateUpdate(a, e, this.subruleIdx), t.apply(this, n);
				let r = this.CST_STACK[this.CST_STACK.length - 1];
				return this.cstPostRule(r), r;
			} catch (e) {
				return this.invokeRuleCatch(e, r, i);
			} finally {
				this.ruleFinallyStateUpdate();
			}
		} : function(...n) {
			try {
				return this.ruleInvocationStateUpdate(a, e, this.subruleIdx), t.apply(this, n);
			} catch (e) {
				return this.invokeRuleCatch(e, r, i);
			} finally {
				this.ruleFinallyStateUpdate();
			}
		}, Object.assign(function(...t) {
			this.onBeforeParse(e);
			try {
				return o.apply(this, t);
			} finally {
				this.onAfterParse(e);
			}
		}, {
			ruleName: e,
			originalGrammarAction: t,
			coreRule: o
		});
	}
	invokeRuleCatch(e, t, n) {
		let r = this.RULE_STACK_IDX === 0, i = t && !this.isBackTracking() && this.recoveryEnabled;
		if (Ym(e)) {
			let t = e;
			if (i) {
				let r = this.findReSyncTokenType();
				if (this.isInCurrentRuleReSyncSet(r)) {
					if (t.resyncedTokens = this.reSyncTo(r), this.outputCst) {
						let e = this.CST_STACK[this.CST_STACK.length - 1];
						return e.recoveredNode = !0, e;
					}
					return n(e);
				}
				if (this.outputCst) {
					let e = this.CST_STACK[this.CST_STACK.length - 1];
					e.recoveredNode = !0, t.partialCstResult = e;
				}
				throw t;
			}
			if (r) return this.moveToTerminatedState(), n(e);
			throw t;
		}
		throw e;
	}
	optionInternal(e, t) {
		let n = this.getKeyForAutomaticLookahead(uh, t);
		return this.optionInternalLogic(e, t, n);
	}
	optionInternalLogic(e, t, n) {
		let r = this.getLaFuncFromCache(n), i;
		if (typeof e != "function") {
			i = e.DEF;
			let t = e.GATE;
			if (t !== void 0) {
				let e = r;
				r = () => t.call(this) && e.call(this);
			}
		} else i = e;
		if (r.call(this) === !0) return i.call(this);
	}
	atLeastOneInternal(e, t) {
		let n = this.getKeyForAutomaticLookahead(fh, e);
		return this.atLeastOneInternalLogic(e, t, n);
	}
	atLeastOneInternalLogic(e, t, n) {
		let r = this.getLaFuncFromCache(n), i;
		if (typeof t != "function") {
			i = t.DEF;
			let e = t.GATE;
			if (e !== void 0) {
				let t = r;
				r = () => e.call(this) && t.call(this);
			}
		} else i = t;
		if (r.call(this) === !0) {
			let e = this.doSingleRepetition(i);
			for (; r.call(this) === !0 && e === !0;) e = this.doSingleRepetition(i);
		} else throw this.raiseEarlyExitException(e, R.REPETITION_MANDATORY, t.ERR_MSG);
		this.attemptInRepetitionRecovery(this.atLeastOneInternal, [e, t], r, fh, e, nm);
	}
	atLeastOneSepFirstInternal(e, t) {
		let n = this.getKeyForAutomaticLookahead(mh, e);
		this.atLeastOneSepFirstInternalLogic(e, t, n);
	}
	atLeastOneSepFirstInternalLogic(e, t, n) {
		let r = t.DEF, i = t.SEP;
		if (this.getLaFuncFromCache(n).call(this) === !0) {
			r.call(this);
			let t = () => this.tokenMatcher(this.LA_FAST(1), i);
			for (; this.tokenMatcher(this.LA_FAST(1), i) === !0;) this.CONSUME(i), r.call(this);
			this.attemptInRepetitionRecovery(this.repetitionSepSecondInternal, [
				e,
				i,
				t,
				r,
				rm
			], t, mh, e, rm);
		} else throw this.raiseEarlyExitException(e, R.REPETITION_MANDATORY_WITH_SEPARATOR, t.ERR_MSG);
	}
	manyInternal(e, t) {
		let n = this.getKeyForAutomaticLookahead(dh, e);
		return this.manyInternalLogic(e, t, n);
	}
	manyInternalLogic(e, t, n) {
		let r = this.getLaFuncFromCache(n), i;
		if (typeof t != "function") {
			i = t.DEF;
			let e = t.GATE;
			if (e !== void 0) {
				let t = r;
				r = () => e.call(this) && t.call(this);
			}
		} else i = t;
		let a = !0;
		for (; r.call(this) === !0 && a === !0;) a = this.doSingleRepetition(i);
		this.attemptInRepetitionRecovery(this.manyInternal, [e, t], r, dh, e, em, a);
	}
	manySepFirstInternal(e, t) {
		let n = this.getKeyForAutomaticLookahead(ph, e);
		this.manySepFirstInternalLogic(e, t, n);
	}
	manySepFirstInternalLogic(e, t, n) {
		let r = t.DEF, i = t.SEP;
		if (this.getLaFuncFromCache(n).call(this) === !0) {
			r.call(this);
			let t = () => this.tokenMatcher(this.LA_FAST(1), i);
			for (; this.tokenMatcher(this.LA_FAST(1), i) === !0;) this.CONSUME(i), r.call(this);
			this.attemptInRepetitionRecovery(this.repetitionSepSecondInternal, [
				e,
				i,
				t,
				r,
				tm
			], t, ph, e, tm);
		}
	}
	repetitionSepSecondInternal(e, t, n, r, i) {
		for (; n();) this.CONSUME(t), r.call(this);
		this.attemptInRepetitionRecovery(this.repetitionSepSecondInternal, [
			e,
			t,
			n,
			r,
			i
		], n, mh, e, i);
	}
	doSingleRepetition(e) {
		let t = this.getLexerPosition();
		return e.call(this), this.getLexerPosition() > t;
	}
	orInternal(e, t) {
		let n = this.getKeyForAutomaticLookahead(lh, t), r = C(e) ? e : e.DEF, i = this.getLaFuncFromCache(n).call(this, r);
		if (i !== void 0) return r[i].ALT.call(this);
		this.raiseNoAltException(t, e.ERR_MSG);
	}
	ruleFinallyStateUpdate() {
		this.RULE_STACK_IDX--, this.RULE_OCCURRENCE_STACK_IDX--, this.RULE_STACK_IDX >= 0 && (this.currRuleShortName = this.RULE_STACK[this.RULE_STACK_IDX]), this.cstFinallyStateUpdate();
	}
	subruleInternal(e, t, n) {
		let r;
		try {
			let i = n === void 0 ? void 0 : n.ARGS;
			return this.subruleIdx = t, r = e.coreRule.apply(this, i), this.cstPostNonTerminal(r, n !== void 0 && n.LABEL !== void 0 ? n.LABEL : e.ruleName), r;
		} catch (t) {
			throw this.subruleInternalError(t, n, e.ruleName);
		}
	}
	subruleInternalError(e, t, n) {
		throw Ym(e) && e.partialCstResult !== void 0 && (this.cstPostNonTerminal(e.partialCstResult, t !== void 0 && t.LABEL !== void 0 ? t.LABEL : n), delete e.partialCstResult), e;
	}
	consumeInternal(e, t, n) {
		let r;
		try {
			let t = this.LA_FAST(1);
			this.tokenMatcher(t, e) === !0 ? (this.consumeToken(), r = t) : this.consumeInternalError(e, t, n);
		} catch (n) {
			r = this.consumeInternalRecovery(e, t, n);
		}
		return this.cstPostTerminal(n !== void 0 && n.LABEL !== void 0 ? n.LABEL : e.name, r), r;
	}
	consumeInternalError(e, t, n) {
		let r, i = this.LA(0);
		throw r = n !== void 0 && n.ERR_MSG ? n.ERR_MSG : this.errorMessageProvider.buildMismatchTokenMessage({
			expected: e,
			actual: t,
			previous: i,
			ruleName: this.getCurrRuleFullName()
		}), this.SAVE_ERROR(new Zm(r, t, i));
	}
	consumeInternalRecovery(e, t, n) {
		if (this.recoveryEnabled && n.name === "MismatchedTokenException" && !this.isBackTracking()) {
			let r = this.getFollowsForInRuleRecovery(e, t);
			try {
				return this.tryInRuleRecovery(e, r);
			} catch (e) {
				throw e.name === nh ? n : e;
			}
		}
		throw n;
	}
	saveRecogState() {
		let e = this.errors, t = this.RULE_STACK.slice(0, this.RULE_STACK_IDX + 1);
		return {
			errors: e,
			lexerState: this.exportLexerState(),
			RULE_STACK: t,
			CST_STACK: this.CST_STACK
		};
	}
	reloadRecogState(e) {
		this.errors = e.errors, this.importLexerState(e.lexerState);
		let t = e.RULE_STACK;
		for (let e = 0; e < t.length; e++) this.RULE_STACK[e] = t[e];
		this.RULE_STACK_IDX = t.length - 1, this.RULE_STACK_IDX >= 0 && (this.currRuleShortName = this.RULE_STACK[this.RULE_STACK_IDX]);
	}
	ruleInvocationStateUpdate(e, t, n) {
		this.RULE_OCCURRENCE_STACK[++this.RULE_OCCURRENCE_STACK_IDX] = n, this.RULE_STACK[++this.RULE_STACK_IDX] = e, this.currRuleShortName = e, this.cstInvocationStateUpdate(t);
	}
	isBackTracking() {
		return this.isBackTrackingStack.length !== 0;
	}
	getCurrRuleFullName() {
		let e = this.currRuleShortName;
		return this.shortRuleNameToFull[e];
	}
	shortRuleNameToFullName(e) {
		return this.shortRuleNameToFull[e];
	}
	isAtEndOfInput() {
		return this.tokenMatcher(this.LA(1), Up);
	}
	reset() {
		this.resetLexerState(), this.subruleIdx = 0, this.currRuleShortName = 0, this.isBackTrackingStack = [], this.errors = [], this.RULE_STACK_IDX = -1, this.RULE_OCCURRENCE_STACK_IDX = -1, this.CST_STACK = [];
	}
	onBeforeParse(e) {
		for (let e = 0; e < this.maxLookahead + 1; e++) this.tokVector.push(Zh);
	}
	onAfterParse(e) {
		if (this.isAtEndOfInput() === !1) {
			let e = this.LA(1), t = this.errorMessageProvider.buildNotAllInputParsedMessage({
				firstRedundant: e,
				ruleName: this.getCurrRuleFullName()
			});
			this.SAVE_ERROR(new $m(t, e));
		}
		for (; this.tokVector.at(-1) === Zh;) this.tokVector.pop();
	}
}, Ih = class {
	initErrorHandler(e) {
		this._errors = [], this.errorMessageProvider = A(e, "errorMessageProvider") ? e.errorMessageProvider : Qh.errorMessageProvider;
	}
	SAVE_ERROR(e) {
		if (Ym(e)) return e.context = {
			ruleStack: this.getHumanReadableRuleStack(),
			ruleOccurrenceStack: this.RULE_OCCURRENCE_STACK.slice(0, this.RULE_OCCURRENCE_STACK_IDX + 1)
		}, this._errors.push(e), e;
		throw Error("Trying to save an Error which is not a RecognitionException");
	}
	get errors() {
		return Qs(this._errors);
	}
	set errors(e) {
		this._errors = e;
	}
	raiseEarlyExitException(e, t, n) {
		let r = this.getCurrRuleFullName(), i = this.getGAstProductions()[r], a = ym(e, i, t, this.maxLookahead)[0], o = [];
		for (let e = 1; e <= this.maxLookahead; e++) o.push(this.LA(e));
		let s = this.errorMessageProvider.buildEarlyExitMessage({
			expectedIterationPaths: a,
			actual: o,
			previous: this.LA(0),
			customUserDescription: n,
			ruleName: r
		});
		throw this.SAVE_ERROR(new eh(s, this.LA(1), this.LA(0)));
	}
	raiseNoAltException(e, t) {
		let n = this.getCurrRuleFullName(), r = this.getGAstProductions()[n], i = vm(e, r, this.maxLookahead), a = [];
		for (let e = 1; e <= this.maxLookahead; e++) a.push(this.LA(e));
		let o = this.LA(0), s = this.errorMessageProvider.buildNoViableAltMessage({
			expectedPathsPerAlt: i,
			actual: a,
			previous: o,
			customUserDescription: t,
			ruleName: this.getCurrRuleFullName()
		});
		throw this.SAVE_ERROR(new Qm(s, this.LA(1), o));
	}
}, Lh = class {
	initContentAssist() {}
	computeContentAssist(e, t) {
		let n = this.gastProductionsCache[e];
		if ($u(n)) throw Error(`Rule ->${e}<- does not exist in this grammar.`);
		return am([n], t, this.tokenMatcher, this.maxLookahead);
	}
	getNextPossibleTokenTypes(e) {
		let t = yu(e.ruleStack), n = this.getGAstProductions()[t];
		return new Qp(n, e).startWalking();
	}
}, Rh = { description: "This Object indicates the Parser is during Recording Phase" };
Object.freeze(Rh);
var zh = !0, Bh = 2 ** sh - 1, Vh = Vp({
	name: "RECORDING_PHASE_TOKEN",
	pattern: L.NA
});
_p([Vh]);
var Hh = Wp(Vh, "This IToken indicates the Parser is in Recording Phase\n	See: https://chevrotain.io/docs/guide/internals.html#grammar-recording for details", -1, -1, -1, -1, -1, -1);
Object.freeze(Hh);
var Uh = {
	name: "This CSTNode indicates the Parser is in Recording Phase\n	See: https://chevrotain.io/docs/guide/internals.html#grammar-recording for details",
	children: {}
}, Wh = class {
	initGastRecorder(e) {
		this.recordingProdStack = [], this.RECORDING_PHASE = !1;
	}
	enableRecording() {
		this.RECORDING_PHASE = !0, this.TRACE_INIT("Enable Recording", () => {
			for (let e = 0; e < 10; e++) {
				let t = e > 0 ? e : "";
				this[`CONSUME${t}`] = function(t, n) {
					return this.consumeInternalRecord(t, e, n);
				}, this[`SUBRULE${t}`] = function(t, n) {
					return this.subruleInternalRecord(t, e, n);
				}, this[`OPTION${t}`] = function(t) {
					return this.optionInternalRecord(t, e);
				}, this[`OR${t}`] = function(t) {
					return this.orInternalRecord(t, e);
				}, this[`MANY${t}`] = function(t) {
					this.manyInternalRecord(e, t);
				}, this[`MANY_SEP${t}`] = function(t) {
					this.manySepFirstInternalRecord(e, t);
				}, this[`AT_LEAST_ONE${t}`] = function(t) {
					this.atLeastOneInternalRecord(e, t);
				}, this[`AT_LEAST_ONE_SEP${t}`] = function(t) {
					this.atLeastOneSepFirstInternalRecord(e, t);
				};
			}
			this.consume = function(e, t, n) {
				return this.consumeInternalRecord(t, e, n);
			}, this.subrule = function(e, t, n) {
				return this.subruleInternalRecord(t, e, n);
			}, this.option = function(e, t) {
				return this.optionInternalRecord(t, e);
			}, this.or = function(e, t) {
				return this.orInternalRecord(t, e);
			}, this.many = function(e, t) {
				this.manyInternalRecord(e, t);
			}, this.atLeastOne = function(e, t) {
				this.atLeastOneInternalRecord(e, t);
			}, this.ACTION = this.ACTION_RECORD, this.BACKTRACK = this.BACKTRACK_RECORD, this.LA = this.LA_RECORD;
		});
	}
	disableRecording() {
		this.RECORDING_PHASE = !1, this.TRACE_INIT("Deleting Recording methods", () => {
			let e = this;
			for (let t = 0; t < 10; t++) {
				let n = t > 0 ? t : "";
				delete e[`CONSUME${n}`], delete e[`SUBRULE${n}`], delete e[`OPTION${n}`], delete e[`OR${n}`], delete e[`MANY${n}`], delete e[`MANY_SEP${n}`], delete e[`AT_LEAST_ONE${n}`], delete e[`AT_LEAST_ONE_SEP${n}`];
			}
			delete e.consume, delete e.subrule, delete e.option, delete e.or, delete e.many, delete e.atLeastOne, delete e.ACTION, delete e.BACKTRACK, delete e.LA;
		});
	}
	ACTION_RECORD(e) {}
	BACKTRACK_RECORD(e, t) {
		return () => !0;
	}
	LA_RECORD(e) {
		return Zh;
	}
	topLevelRuleRecord(e, t) {
		try {
			let n = new Nd({
				definition: [],
				name: e
			});
			return n.name = e, this.recordingProdStack.push(n), t.call(this), this.recordingProdStack.pop(), n;
		} catch (e) {
			if (e.KNOWN_RECORDER_ERROR !== !0) try {
				e.message += "\n	 This error was thrown during the \"grammar recording phase\" For more info see:\n	https://chevrotain.io/docs/guide/internals.html#grammar-recording";
			} catch {
				throw e;
			}
			throw e;
		}
	}
	optionInternalRecord(e, t) {
		return Gh.call(this, Fd, e, t);
	}
	atLeastOneInternalRecord(e, t) {
		Gh.call(this, Id, t, e);
	}
	atLeastOneSepFirstInternalRecord(e, t) {
		Gh.call(this, Ld, t, e, zh);
	}
	manyInternalRecord(e, t) {
		Gh.call(this, N, t, e);
	}
	manySepFirstInternalRecord(e, t) {
		Gh.call(this, Rd, t, e, zh);
	}
	orInternalRecord(e, t) {
		return Kh.call(this, e, t);
	}
	subruleInternalRecord(e, t, n) {
		if (Jh(t), !e || A(e, "ruleName") === !1) {
			let n = /* @__PURE__ */ Error(`<SUBRULE${qh(t)}> argument is invalid expecting a Parser method reference but got: <${JSON.stringify(e)}>
 inside top level rule: <${this.recordingProdStack[0].name}>`);
			throw n.KNOWN_RECORDER_ERROR = !0, n;
		}
		let r = Xl(this.recordingProdStack), i = e.ruleName, a = new Md({
			idx: t,
			nonTerminalName: i,
			label: n?.LABEL,
			referencedRule: void 0
		});
		return r.definition.push(a), this.outputCst ? Uh : Rh;
	}
	consumeInternalRecord(e, t, n) {
		if (Jh(t), !Cp(e)) {
			let n = /* @__PURE__ */ Error(`<CONSUME${qh(t)}> argument is invalid expecting a TokenType reference but got: <${JSON.stringify(e)}>
 inside top level rule: <${this.recordingProdStack[0].name}>`);
			throw n.KNOWN_RECORDER_ERROR = !0, n;
		}
		let r = Xl(this.recordingProdStack), i = new P({
			idx: t,
			terminalType: e,
			label: n?.LABEL
		});
		return r.definition.push(i), Hh;
	}
};
function Gh(e, t, n, r = !1) {
	Jh(n);
	let i = Xl(this.recordingProdStack), a = Ie(t) ? t : t.DEF, o = new e({
		definition: [],
		idx: n
	});
	return r && (o.separator = t.SEP), A(t, "MAX_LOOKAHEAD") && (o.maxLookahead = t.MAX_LOOKAHEAD), this.recordingProdStack.push(o), a.call(this), i.definition.push(o), this.recordingProdStack.pop(), Rh;
}
function Kh(e, t) {
	Jh(t);
	let n = Xl(this.recordingProdStack), r = C(e) === !1, i = r === !1 ? e : e.DEF, a = new zd({
		definition: [],
		idx: t,
		ignoreAmbiguities: r && e.IGNORE_AMBIGUITIES === !0
	});
	return A(e, "MAX_LOOKAHEAD") && (a.maxLookahead = e.MAX_LOOKAHEAD), a.hasPredicates = vd(i, (e) => Ie(e.GATE)), n.definition.push(a), O(i, (e) => {
		let t = new Pd({ definition: [] });
		a.definition.push(t), A(e, "IGNORE_AMBIGUITIES") ? t.ignoreAmbiguities = e.IGNORE_AMBIGUITIES : A(e, "GATE") && (t.ignoreAmbiguities = !0), this.recordingProdStack.push(t), e.ALT.call(this), this.recordingProdStack.pop();
	}), Rh;
}
function qh(e) {
	return e === 0 ? "" : `${e}`;
}
function Jh(e) {
	if (e < 0 || e > Bh) {
		let t = /* @__PURE__ */ Error(`Invalid DSL Method idx value: <${e}>
	Idx value must be a none negative value smaller than ${Bh + 1}`);
		throw t.KNOWN_RECORDER_ERROR = !0, t;
	}
}
var Yh = class {
	initPerformanceTracer(e) {
		if (A(e, "traceInitPerf")) {
			let t = e.traceInitPerf, n = typeof t == "number";
			this.traceInitMaxIdent = n ? t : Infinity, this.traceInitPerf = n ? t > 0 : t;
		} else this.traceInitMaxIdent = 0, this.traceInitPerf = Qh.traceInitPerf;
		this.traceInitIndent = -1;
	}
	TRACE_INIT(e, t) {
		if (this.traceInitPerf === !0) {
			this.traceInitIndent++;
			let n = Array(this.traceInitIndent + 1).join("	");
			this.traceInitIndent < this.traceInitMaxIdent && console.log(`${n}--> <${e}>`);
			let { time: r, value: i } = Dd(t), a = r > 10 ? console.warn : console.log;
			return this.traceInitIndent < this.traceInitMaxIdent && a(`${n}<-- <${e}> time: ${r}ms`), this.traceInitIndent--, i;
		}
		return t();
	}
};
function Xh(e, t) {
	t.forEach((t) => {
		let n = t.prototype;
		Object.getOwnPropertyNames(n).forEach((r) => {
			if (r === "constructor") return;
			let i = Object.getOwnPropertyDescriptor(n, r);
			i && (i.get || i.set) ? Object.defineProperty(e.prototype, r, i) : e.prototype[r] = t.prototype[r];
		});
	});
}
var Zh = Wp(Up, "", NaN, NaN, NaN, NaN, NaN, NaN);
Object.freeze(Zh);
var Qh = Object.freeze({
	recoveryEnabled: !1,
	maxLookahead: 3,
	dynamicTokensEnabled: !1,
	outputCst: !0,
	errorMessageProvider: Kp,
	nodeLocationTracking: "none",
	traceInitPerf: !1,
	skipValidations: !1
}), $h = Object.freeze({
	recoveryValueFunc: () => void 0,
	resyncEnabled: !0
}), eg;
(function(e) {
	e[e.INVALID_RULE_NAME = 0] = "INVALID_RULE_NAME", e[e.DUPLICATE_RULE_NAME = 1] = "DUPLICATE_RULE_NAME", e[e.INVALID_RULE_OVERRIDE = 2] = "INVALID_RULE_OVERRIDE", e[e.DUPLICATE_PRODUCTIONS = 3] = "DUPLICATE_PRODUCTIONS", e[e.UNRESOLVED_SUBRULE_REF = 4] = "UNRESOLVED_SUBRULE_REF", e[e.LEFT_RECURSION = 5] = "LEFT_RECURSION", e[e.NONE_LAST_EMPTY_ALT = 6] = "NONE_LAST_EMPTY_ALT", e[e.AMBIGUOUS_ALTS = 7] = "AMBIGUOUS_ALTS", e[e.CONFLICT_TOKENS_RULES_NAMESPACE = 8] = "CONFLICT_TOKENS_RULES_NAMESPACE", e[e.INVALID_TOKEN_NAME = 9] = "INVALID_TOKEN_NAME", e[e.NO_NON_EMPTY_LOOKAHEAD = 10] = "NO_NON_EMPTY_LOOKAHEAD", e[e.AMBIGUOUS_PREFIX_ALTS = 11] = "AMBIGUOUS_PREFIX_ALTS", e[e.TOO_MANY_ALTS = 12] = "TOO_MANY_ALTS", e[e.CUSTOM_LOOKAHEAD_VALIDATION = 13] = "CUSTOM_LOOKAHEAD_VALIDATION";
})(eg ||= {});
var tg = class e {
	static performSelfAnalysis(e) {
		throw Error("The **static** `performSelfAnalysis` method has been deprecated.	\nUse the **instance** method with the same name instead.");
	}
	performSelfAnalysis() {
		this.TRACE_INIT("performSelfAnalysis", () => {
			let t;
			this.selfAnalysisDone = !0;
			let n = this.className;
			this.TRACE_INIT("toFastProps", () => {
				Od(this);
			}), this.TRACE_INIT("Grammar Recording", () => {
				try {
					this.enableRecording(), O(this.definedRulesNames, (e) => {
						let t = this[e].originalGrammarAction, n;
						this.TRACE_INIT(`${e} Rule`, () => {
							n = this.topLevelRuleRecord(e, t);
						}), this.gastProductionsCache[e] = n;
					});
				} finally {
					this.disableRecording();
				}
			});
			let r = [];
			if (this.TRACE_INIT("Grammar Resolving", () => {
				r = Hm({ rules: j(this.gastProductionsCache) }), this.definitionErrors = this.definitionErrors.concat(r);
			}), this.TRACE_INIT("Grammar Validations", () => {
				if (M(r) && this.skipValidations === !1) {
					let e = Um({
						rules: j(this.gastProductionsCache),
						tokenTypes: j(this.tokensMap),
						errMsgProvider: Jp,
						grammarName: n
					}), t = Cm({
						lookaheadStrategy: this.lookaheadStrategy,
						rules: j(this.gastProductionsCache),
						tokenTypes: j(this.tokensMap),
						grammarName: n
					});
					this.definitionErrors = this.definitionErrors.concat(e, t);
				}
			}), M(this.definitionErrors) && (this.recoveryEnabled && this.TRACE_INIT("computeAllProdsFollows", () => {
				let e = tf(j(this.gastProductionsCache));
				this.resyncFollows = e;
			}), this.TRACE_INIT("ComputeLookaheadFunctions", () => {
				var e, t;
				(t = (e = this.lookaheadStrategy).initialize) == null || t.call(e, { rules: j(this.gastProductionsCache) }), this.preComputeLookaheadFunctions(j(this.gastProductionsCache));
			})), !e.DEFER_DEFINITION_ERRORS_HANDLING && !M(this.definitionErrors)) throw t = k(this.definitionErrors, (e) => e.message), Error(`Parser Definition Errors detected:
 ${t.join("\n-------------------------------\n")}`);
		});
	}
	constructor(e, t) {
		this.definitionErrors = [], this.selfAnalysisDone = !1;
		let n = this;
		if (n.initErrorHandler(t), n.initLexerAdapter(), n.initLooksAhead(t), n.initRecognizerEngine(e, t), n.initRecoverable(t), n.initTreeBuilder(t), n.initContentAssist(), n.initGastRecorder(t), n.initPerformanceTracer(t), A(t, "ignoredIssues")) throw Error("The <ignoredIssues> IParserConfig property has been deprecated.\n	Please use the <IGNORE_AMBIGUITIES> flag on the relevant DSL method instead.\n	See: https://chevrotain.io/docs/guide/resolving_grammar_errors.html#IGNORING_AMBIGUITIES\n	For further details.");
		this.skipValidations = A(t, "skipValidations") ? t.skipValidations : Qh.skipValidations;
	}
};
tg.DEFER_DEFINITION_ERRORS_HANDLING = !1, Xh(tg, [
	ih,
	_h,
	Mh,
	Nh,
	Fh,
	Ph,
	Ih,
	Lh,
	Wh,
	Yh
]);
var ng = class extends tg {
	constructor(e, t = Qh) {
		let n = Qs(t);
		n.outputCst = !1, super(e, n);
	}
};
//#endregion
//#region ../../wt-query-graph/web/node_modules/@traqula/core/dist/esm/lib/utils.js
function z(e) {
	return e.charAt(0).toLowerCase() + e.slice(1);
}
function B(e) {
	return Vp(e);
}
var V = "When you use this string, you expect traqula to handle indentation after every newline", rg = "When you use this string, you expect that the core generator of Traqula does prints a given string instead of a ewline when NEW_LINE printing is disabled through traqulaIndentation", ig = class {
	rules;
	factory = new e();
	__context = void 0;
	origSource = "";
	handledInlineSource;
	generatedUntil = 0;
	toEnsure = [];
	stringBuilder = [];
	constructor(e) {
		this.rules = e;
		for (let t of Object.values(e)) this[t.name] = ((e, n, r) => (this.stringBuilder.length = 0, this.toEnsure.length = 0, this.origSource = n.origSource, this.generatedUntil = n?.offset ?? 0, this.setContext(n), this.subrule(t, e, r), this.catchup(this.origSource.length), this.handeEnsured(""), this.stringBuilder.join("")));
	}
	setContext(e) {
		this.__context = e;
	}
	getSafeContext() {
		return this.__context;
	}
	subrule = (e, t, ...n) => {
		let r = this.rules[e.name];
		if (!r) throw Error(`Rule ${e.name} not found`);
		let i = () => r.gImpl({
			SUBRULE: this.subrule,
			PRINT: this.print,
			ENSURE: this.ensure,
			ENSURE_EITHER: this.ensureEither,
			NEW_LINE: this.newLine,
			HANDLE_LOC: this.handleLoc,
			CATCHUP: this.catchup,
			PRINT_WORD: this.printWord,
			PRINT_WORDS: this.printWords,
			PRINT_ON_EMPTY: this.printOnEmpty,
			PRINT_ON_OWN_LINE: this.printOnOwnLine
		})(t, this.getSafeContext(), ...n);
		this.factory.isLocalized(t) ? this.handleLoc(t, i) : i();
	};
	handleLoc = (e, t) => {
		if (this.factory.isSourceLocationNoMaterialize(e.loc)) return;
		if (this.factory.isSourceLocationStringReplace(e.loc)) {
			this.catchup(e.loc.start), this.print(e.loc.newSource), this.generatedUntil = e.loc.end;
			return;
		}
		if (this.factory.isSourceLocationNodeReplace(e.loc) && (this.catchup(e.loc.start), this.generatedUntil = e.loc.end), this.factory.isSourceLocationSource(e.loc) && this.catchup(e.loc.start), this.factory.isSourceLocationInlinedSource(e.loc) && this.handledInlineSource !== e.loc) {
			this.handledInlineSource = e.loc, this.catchup(e.loc.start);
			let n = this.origSource, r = this.generatedUntil;
			this.origSource = e.loc.newSource, this.generatedUntil = 0, this.catchup(e.loc.startOnNew);
			let i = this.handleLoc(e.loc, t);
			return this.generatedUntil = e.loc.endOnNew, this.catchup(this.origSource.length), this.origSource = n, this.generatedUntil = Math.max(r, e.loc.end), i;
		}
		let n = t();
		return this.factory.isSourceLocationSource(e.loc) && this.catchup(e.loc.end), n;
	};
	catchup = (e) => {
		let t = this.generatedUntil;
		t < e && this.print(this.origSource.slice(t, e)), this.generatedUntil = Math.max(this.generatedUntil, e);
	};
	handeEnsured(e) {
		for (let t of this.toEnsure) t(e);
		this.toEnsure.length = 0;
	}
	print = (...e) => {
		let t = e.join("");
		this.handeEnsured(t), this.stringBuilder.push(t);
	};
	doesEndWith(e) {
		let t = e.length, n = "";
		for (; n.length < t && this.stringBuilder.length > 0;) n = this.stringBuilder.pop() + n;
		return this.stringBuilder.push(n), n.endsWith(e);
	}
	ensure = (...e) => {
		let t = e.join("");
		this.doesEndWith(t) || this.toEnsure.push((e) => {
			!e.startsWith(t) && !this.doesEndWith(t) && this.stringBuilder.push(t);
		});
	};
	ensureEither = (...e) => {
		e.length === 1 ? this.ensure(...e) : e.length > 1 && !e.some((e) => this.doesEndWith(e)) && this.toEnsure.push((t) => {
			!e.some((e) => t.startsWith(e)) && !e.some((e) => this.doesEndWith(e)) && this.stringBuilder.push(e[0]);
		});
	};
	pruneEndingBlanks() {
		let e = "";
		for (; /^[ \t]*$/u.test(e) && this.stringBuilder.length > 0;) e = this.stringBuilder.pop() + e;
		this.print(e.replace(/[\t ]*$/u, ""));
	}
	newLine = (e) => {
		let t = this.getSafeContext()["When you use this string, you expect traqula to handle indentation after every newline"] ?? 0, n = e?.force ?? !1;
		if (t < 0) {
			let e = this.getSafeContext()[rg];
			e !== void 0 && (n || this.stringBuilder.at(-1) !== e) && this.print(e);
			return;
		}
		if (this.pruneEndingBlanks(), n) this.print("\n", " ".repeat(t));
		else {
			let e = "";
			for (; !e.includes("\n") && this.stringBuilder.length > 0;) e = this.stringBuilder.pop() + e;
			/\n[ \t]*$/u.test(e) ? (e = e.replace(/\n[ \t]*$/u, `\n${" ".repeat(t)}`), this.print(e)) : this.print(e, "\n", " ".repeat(t));
		}
	};
	printWord = (...e) => {
		this.ensureEither(" ", "\n"), this.print(...e), this.ensureEither(" ", "\n");
	};
	printWords = (...e) => {
		for (let t of e) this.printWord(t);
	};
	printOnEmpty = (...e) => {
		this.newLine(), this.print(...e);
	};
	printOnOwnLine = (...e) => {
		this.newLine(), this.print(...e), this.newLine();
	};
};
//#endregion
//#region ../../wt-query-graph/web/node_modules/@traqula/core/dist/esm/lib/generator-builder/generatorBuilder.js
function ag(e) {
	let t = Object.create(null);
	for (let n of e) t[n.name] = n;
	return t;
}
var og = class e {
	static create(t) {
		return Array.isArray(t) ? new e(ag(t)) : new e({ ...t.rules });
	}
	rules;
	constructor(e) {
		this.rules = e;
	}
	widenContext() {
		return this;
	}
	typePatch() {
		return this;
	}
	patchRule(e) {
		let t = this;
		return t.rules[e.name] = e, t;
	}
	addRuleRedundant(e) {
		let t = this, n = t.rules;
		if (n[e.name] !== void 0 && n[e.name] !== e) throw Error(`Rule ${e.name} already exists in the GeneratorBuilder`);
		return n[e.name] = e, t;
	}
	addRule(e) {
		return this.addRuleRedundant(e);
	}
	addMany(...e) {
		return this.rules = {
			...this.rules,
			...ag(e)
		}, this;
	}
	deleteRule(e) {
		return delete this.rules[e], this;
	}
	deleteMany(...e) {
		for (let t of e) delete this.rules[t];
		return this;
	}
	getRule(e) {
		return this.rules[e];
	}
	merge(e, t) {
		let n = Object.assign(Object.create(null), e.rules), r = this.rules;
		for (let e of Object.values(r)) if (n[e.name] === void 0) n[e.name] = e;
		else if (n[e.name] !== e) {
			let r = t.find((t) => t.name === e.name);
			if (r) n[e.name] = r;
			else throw Error(`Rule with name "${e.name}" already exists in the GeneratorBuilder, specify an override to resolve conflict`);
		}
		return this.rules = n, this;
	}
	build() {
		return new ig(this.rules);
	}
}, sg = class e {
	tokens;
	static create(t) {
		return new e(t);
	}
	constructor(e) {
		this.tokens = e?.tokens ? [...e.tokens] : [];
	}
	merge(e, t = []) {
		let n = e.tokens.filter((e) => {
			if (t.find((t) => t.name === e.name)) return !1;
			let n = this.tokens.find((t) => t.name === e.name);
			if (n) {
				if (n !== e) throw Error(`Token with name ${e.name} already exists. Implementation is different and no overwrite was provided.`);
				return !1;
			}
			return !0;
		});
		return this.tokens.push(...n), this;
	}
	add(...e) {
		return this.tokens.push(...e), this;
	}
	addBefore(e, ...t) {
		let n = this.tokens.indexOf(e);
		if (n === -1) throw Error("Token not found");
		return this.tokens.splice(n, 0, ...t), this;
	}
	moveBeforeOrAfter(e, t, ...n) {
		let r = this.tokens.indexOf(t) + (e === "before" ? 0 : 1);
		if (r === -1) throw Error("BeforeToken not found");
		for (let e of n) {
			let t = this.tokens.indexOf(e);
			if (t === -1) throw Error("Token not found");
			this.tokens.splice(t, 1), this.tokens.splice(r, 0, e);
		}
		return this;
	}
	moveBefore(e, ...t) {
		return this.moveBeforeOrAfter("before", e, ...t);
	}
	moveAfter(e, ...t) {
		return this.moveBeforeOrAfter("after", e, ...t);
	}
	addAfter(e, ...t) {
		let n = this.tokens.indexOf(e);
		if (n === -1) throw Error("Token not found");
		return this.tokens.splice(n + 1, 0, ...t), this;
	}
	delete(...e) {
		for (let t of e) {
			let e = this.tokens.indexOf(t);
			if (e === -1) throw Error("Token not found");
			this.tokens.splice(e, 1);
		}
		return this;
	}
	deleteToken(...e) {
		for (let t of e) {
			let e = this.tokens.findIndex((e) => e.name === t);
			if (e === -1) throw Error(`Token with name "${t}" not found`);
			this.tokens.splice(e, 1);
		}
		return this;
	}
	replace(e) {
		let t = this.tokens.findIndex((t) => t.name === e.name);
		if (t === -1) throw Error(`Token with name "${e.name}" not found`);
		return this.tokens[t] = e, this;
	}
	build(e) {
		return new L(this.tokens, {
			positionTracking: "onlyStart",
			recoveryEnabled: !1,
			ensureOptimizations: !0,
			...e
		});
	}
	get tokenVocabulary() {
		return this.tokens;
	}
}, cg = class extends ng {
	context;
	setContext(e) {
		this.context = e;
	}
	constructor(e, t, n = {}) {
		super(t, {
			maxLookahead: 1,
			skipValidations: !0,
			dynamicTokensEnabled: !1,
			...n
		}), this.context = void 0;
		let r = {
			...this.constructSelfRef(),
			cache: /* @__PURE__ */ new WeakMap()
		};
		for (let t of Object.values(e)) this[t.name] = this.RULE(t.name, t.impl(r));
		this.performSelfAnalysis();
	}
	constructSelfRef() {
		let e = (e) => ((t, ...n) => e(this[t.name], { ARGS: [this.context, ...n] }));
		return {
			CONSUME: (e, t) => this.CONSUME(e, t),
			CONSUME1: (e, t) => this.CONSUME1(e, t),
			CONSUME2: (e, t) => this.CONSUME2(e, t),
			CONSUME3: (e, t) => this.CONSUME3(e, t),
			CONSUME4: (e, t) => this.CONSUME4(e, t),
			CONSUME5: (e, t) => this.CONSUME5(e, t),
			CONSUME6: (e, t) => this.CONSUME6(e, t),
			CONSUME7: (e, t) => this.CONSUME7(e, t),
			CONSUME8: (e, t) => this.CONSUME8(e, t),
			CONSUME9: (e, t) => this.CONSUME9(e, t),
			OPTION: (e) => this.OPTION(e),
			OPTION1: (e) => this.OPTION1(e),
			OPTION2: (e) => this.OPTION2(e),
			OPTION3: (e) => this.OPTION3(e),
			OPTION4: (e) => this.OPTION4(e),
			OPTION5: (e) => this.OPTION5(e),
			OPTION6: (e) => this.OPTION6(e),
			OPTION7: (e) => this.OPTION7(e),
			OPTION8: (e) => this.OPTION8(e),
			OPTION9: (e) => this.OPTION9(e),
			OR: (e) => this.OR(e),
			OR1: (e) => this.OR1(e),
			OR2: (e) => this.OR2(e),
			OR3: (e) => this.OR3(e),
			OR4: (e) => this.OR4(e),
			OR5: (e) => this.OR5(e),
			OR6: (e) => this.OR6(e),
			OR7: (e) => this.OR7(e),
			OR8: (e) => this.OR8(e),
			OR9: (e) => this.OR9(e),
			MANY: (e) => this.MANY(e),
			MANY1: (e) => this.MANY1(e),
			MANY2: (e) => this.MANY2(e),
			MANY3: (e) => this.MANY3(e),
			MANY4: (e) => this.MANY4(e),
			MANY5: (e) => this.MANY5(e),
			MANY6: (e) => this.MANY6(e),
			MANY7: (e) => this.MANY7(e),
			MANY8: (e) => this.MANY8(e),
			MANY9: (e) => this.MANY9(e),
			MANY_SEP: (e) => this.MANY_SEP(e),
			MANY_SEP1: (e) => this.MANY_SEP1(e),
			MANY_SEP2: (e) => this.MANY_SEP2(e),
			MANY_SEP3: (e) => this.MANY_SEP3(e),
			MANY_SEP4: (e) => this.MANY_SEP4(e),
			MANY_SEP5: (e) => this.MANY_SEP5(e),
			MANY_SEP6: (e) => this.MANY_SEP6(e),
			MANY_SEP7: (e) => this.MANY_SEP7(e),
			MANY_SEP8: (e) => this.MANY_SEP8(e),
			MANY_SEP9: (e) => this.MANY_SEP9(e),
			AT_LEAST_ONE: (e) => this.AT_LEAST_ONE(e),
			AT_LEAST_ONE1: (e) => this.AT_LEAST_ONE1(e),
			AT_LEAST_ONE2: (e) => this.AT_LEAST_ONE2(e),
			AT_LEAST_ONE3: (e) => this.AT_LEAST_ONE3(e),
			AT_LEAST_ONE4: (e) => this.AT_LEAST_ONE4(e),
			AT_LEAST_ONE5: (e) => this.AT_LEAST_ONE5(e),
			AT_LEAST_ONE6: (e) => this.AT_LEAST_ONE6(e),
			AT_LEAST_ONE7: (e) => this.AT_LEAST_ONE7(e),
			AT_LEAST_ONE8: (e) => this.AT_LEAST_ONE8(e),
			AT_LEAST_ONE9: (e) => this.AT_LEAST_ONE9(e),
			AT_LEAST_ONE_SEP: (e) => this.AT_LEAST_ONE_SEP(e),
			AT_LEAST_ONE_SEP1: (e) => this.AT_LEAST_ONE_SEP1(e),
			AT_LEAST_ONE_SEP2: (e) => this.AT_LEAST_ONE_SEP2(e),
			AT_LEAST_ONE_SEP3: (e) => this.AT_LEAST_ONE_SEP3(e),
			AT_LEAST_ONE_SEP4: (e) => this.AT_LEAST_ONE_SEP4(e),
			AT_LEAST_ONE_SEP5: (e) => this.AT_LEAST_ONE_SEP5(e),
			AT_LEAST_ONE_SEP6: (e) => this.AT_LEAST_ONE_SEP6(e),
			AT_LEAST_ONE_SEP7: (e) => this.AT_LEAST_ONE_SEP7(e),
			AT_LEAST_ONE_SEP8: (e) => this.AT_LEAST_ONE_SEP8(e),
			AT_LEAST_ONE_SEP9: (e) => this.AT_LEAST_ONE_SEP9(e),
			ACTION: (e) => this.ACTION(e),
			BACKTRACK: (e, ...t) => this.BACKTRACK(this[e.name], { ARGS: t }),
			SUBRULE: e((e, t) => this.SUBRULE(e, t)),
			SUBRULE1: e((e, t) => this.SUBRULE1(e, t)),
			SUBRULE2: e((e, t) => this.SUBRULE2(e, t)),
			SUBRULE3: e((e, t) => this.SUBRULE3(e, t)),
			SUBRULE4: e((e, t) => this.SUBRULE4(e, t)),
			SUBRULE5: e((e, t) => this.SUBRULE5(e, t)),
			SUBRULE6: e((e, t) => this.SUBRULE6(e, t)),
			SUBRULE7: e((e, t) => this.SUBRULE7(e, t)),
			SUBRULE8: e((e, t) => this.SUBRULE8(e, t)),
			SUBRULE9: e((e, t) => this.SUBRULE9(e, t))
		};
	}
};
//#endregion
//#region ../../wt-query-graph/web/node_modules/@traqula/core/dist/esm/lib/parser-builder/parserBuilder.js
function lg(e) {
	let t = Object.create(null);
	for (let n of e) t[n.name] = n;
	return t;
}
var ug = class e {
	static create(t) {
		return Array.isArray(t) ? new e(lg(t)) : new e({ ...t.rules });
	}
	rules;
	constructor(e) {
		this.rules = e;
	}
	widenContext() {
		return this;
	}
	typePatch() {
		return this;
	}
	patchRule(e) {
		let t = this;
		return t.rules[e.name] = e, t;
	}
	addRuleRedundant(e) {
		let t = this, n = t.rules;
		if (n[e.name] !== void 0 && n[e.name] !== e) throw Error(`Rule ${e.name} already exists in the builder`);
		return n[e.name] = e, t;
	}
	addRule(e) {
		return this.addRuleRedundant(e);
	}
	addMany(...e) {
		return this.rules = {
			...this.rules,
			...lg(e)
		}, this;
	}
	deleteRule(e) {
		return delete this.rules[e], this;
	}
	deleteMany(...e) {
		for (let t of e) delete this.rules[t];
		return this;
	}
	getRule(e) {
		return this.rules[e];
	}
	merge(e, t) {
		let n = Object.assign(Object.create(null), e.rules), r = this.rules;
		for (let e of Object.values(r)) if (n[e.name] === void 0) n[e.name] = e;
		else if (n[e.name] !== e) {
			let r = t.find((t) => t.name === e.name);
			if (r) n[e.name] = r;
			else throw Error(`Rule with name "${e.name}" already exists in the builder, specify an override to resolve conflict`);
		}
		return this.rules = n, this;
	}
	defaultErrorHandler(e, t) {
		let n = t[0], r = ["Parse error"], i = n.token.startLine;
		if (i !== void 0 && !Number.isNaN(i)) {
			let t = e.split("\n")[i - 1];
			r.push(` on line ${i}
${t}`);
			let a = n.token.startColumn;
			a !== void 0 && r.push(`\n${"-".repeat(a - 1)}^`);
		}
		throw r.push(`\n${n.message}`), Error(r.join(""));
	}
	build({ tokenVocabulary: e, parserConfig: t = {}, lexerConfig: n = {}, queryPreProcessor: r = (e) => e, errorHandler: i }) {
		let a = sg.create().add(...e).build({
			positionTracking: "onlyOffset",
			recoveryEnabled: !1,
			ensureOptimizations: !0,
			safeMode: !1,
			skipValidations: !0,
			...n
		}), o = this.consume({
			tokenVocabulary: e,
			config: t
		}), s = {};
		for (let e of Object.values(this.rules)) s[e.name] = ((t, n, ...s) => {
			let c = r(t), l = a.tokenize(c);
			if (l.errors.length > 0) throw Error(l.errors[0].message);
			o.input = l.tokens, o.setContext(n);
			let u = o[e.name](n, ...s);
			return o.errors.length > 0 && (i ? i(o.errors) : this.defaultErrorHandler(c, o.errors)), u;
		});
		return s;
	}
	consume({ tokenVocabulary: e, config: t = {} }) {
		return new cg(this.rules, e, t);
	}
};
//#endregion
//#region ../../wt-query-graph/web/node_modules/@traqula/core/dist/esm/lib/transformers/TransformerObject.js
function dg(e) {
	return e instanceof Promise;
}
var fg = class e {
	defaultContext;
	maxStackSize = 1e6;
	maxNodeRewrites = 1e3;
	constructor(e = {}) {
		this.defaultContext = e;
	}
	clone(t = {}) {
		return new e({
			...this.defaultContext,
			...t
		});
	}
	cloneObj(e) {
		if (typeof e != "object" || !e) return e;
		let t = Object.getPrototypeOf(e);
		return t === Object.prototype || t === null ? { ...e } : Object.assign(Object.create(t), e);
	}
	transformObject(e, t, n = () => ({})) {
		return this.runTransformObject(e, t, n, !1);
	}
	transformObjectAsync(e, t, n = () => ({})) {
		return Promise.resolve(this.runTransformObject(e, t, n, !0));
	}
	runTransformObject(e, t, n, r) {
		let i = this.defaultContext, a = i.copy ?? !0, o = i.continue ?? !0, s = i.ignoreKeys, c = i.shallowKeys, l = i.shortcut ?? !1, u = !1, d = { res: e }, f = [e], p = [d], m = ["res"], h = [], g = [], _ = [], v = [], y = [];
		function b() {
			for (; f.length === h.at(-1);) {
				h.pop();
				let e = g.pop(), n = _.pop(), i = v.pop(), a = y.pop(), o = t(e, n);
				if (r && dg(o)) return o.then((e) => (i[a] = e, b()));
				i[a] = o;
			}
		}
		let ee = (e, t, n, r) => {
			let i = r.copy ?? a, d = r.continue ?? o, b = r.ignoreKeys ?? s, ee = r.shallowKeys ?? c;
			u = r.shortcut ?? l;
			let x = i ? this.cloneObj(e) : e;
			if (h.push(f.length), g.push(x), _.push(e), v.push(t), y.push(n), d && !u) for (let e in x) {
				if (!Object.hasOwn(x, e)) continue;
				let t = x[e], n = ee && ee?.has(e);
				n && (x[e] = this.cloneObj(t)), !(b && b.has(e)) && !n && typeof t == "object" && t && (f.push(t), m.push(e), p.push(x));
			}
		}, x = () => {
			for (; f.length > 0 && f.length < this.maxStackSize;) {
				let e = f.pop(), t = p.pop(), i = m.pop();
				if (!u) {
					if (Array.isArray(e)) {
						let n = [...e];
						h.push(f.length), g.push(n), _.push(e), v.push(t), y.push(i);
						for (let t = e.length - 1; t >= 0; t--) {
							let r = e[t];
							typeof r == "object" && r && (f.push(r), p.push(n), m.push(t.toString()));
						}
						let r = b();
						if (r) return r.then(x);
						continue;
					}
					let a = n(e);
					if (r && dg(a)) return a.then((n) => {
						ee(e, t, i, n);
						let r = b();
						return r ? r.then(x) : x();
					});
					ee(e, t, i, a);
				}
				let a = b();
				if (a) return a.then(x);
			}
			if (f.length >= this.maxStackSize) throw Error("Transform object stack overflowed");
			return d.res;
		};
		return x();
	}
	transformObjectPreOrder(e, t) {
		return this.runTransformObjectPreOrder(e, t, !1);
	}
	transformObjectPreOrderAsync(e, t) {
		return Promise.resolve(this.runTransformObjectPreOrder(e, t, !0));
	}
	runTransformObjectPreOrder(e, t, n) {
		let r = this.defaultContext, i = r.copy ?? !0, a = r.continue ?? !0, o = r.ignoreKeys, s = r.shallowKeys, c = r.shortcut ?? !1, l = r.reTransform ?? !1, u = !1, d = { res: e }, f = [e], p = [d], m = ["res"], h = [0];
		function g(e, t) {
			for (let n = e.length - 1; n >= 0; n--) {
				let r = e[n];
				typeof r == "object" && r && (f.push(r), p.push(e), m.push(n.toString()), h.push(t));
			}
		}
		let _ = (e, t, n, r) => {
			let i = e.newValue;
			t[n] = i;
			let d = e.continue ?? a, _ = e.ignoreKeys ?? o, v = e.shallowKeys ?? s, y = e.reTransform ?? l;
			if (u = e.shortcut ?? c, !d || u || typeof i != "object" || !i) return;
			if (Array.isArray(i)) {
				g(i, r + 1);
				return;
			}
			if (y) {
				f.push(i), p.push(t), m.push(n), h.push(r + 1);
				return;
			}
			let b = i;
			for (let e in b) {
				if (!Object.hasOwn(b, e) || _?.has(e)) continue;
				let t = b[e];
				typeof t == "object" && t && (v?.has(e) ? b[e] = this.cloneObj(t) : (f.push(t), p.push(b), m.push(e), h.push(0)));
			}
		}, v = () => {
			for (; !u && f.length > 0 && f.length < this.maxStackSize;) {
				let e = f.pop(), r = p.pop(), a = m.pop(), o = h.pop();
				if (o >= this.maxNodeRewrites) throw Error(`Pre order transform did not converge: rewrote the same position ${this.maxNodeRewrites} times.`, { cause: e });
				if (Array.isArray(e)) {
					let t = [...e];
					r[a] = t, g(t, o);
					continue;
				}
				let s = t(i ? this.cloneObj(e) : e, e);
				if (n && dg(s)) return s.then((e) => (_(e, r, a, o), v()));
				_(s, r, a, o);
			}
			if (f.length >= this.maxStackSize) throw Error("Transform object stack overflowed");
			return d.res;
		};
		return v();
	}
	visitObject(e, t, n = () => ({})) {
		this.runVisitObject(e, t, n, !1);
	}
	visitObjectAsync(e, t, n = () => ({})) {
		return Promise.resolve(this.runVisitObject(e, t, n, !0));
	}
	runVisitObject(e, t, n, r) {
		let i = this.defaultContext, a = i.continue ?? !0, o = i.ignoreKeys, s = i.shortcut ?? !1, c = !1, l = [e], u = [], d = [];
		function f() {
			for (; l.length === u.at(-1);) {
				u.pop();
				let e = t(d.pop());
				if (r && dg(e)) return e.then(() => f());
			}
		}
		let p = (e, t) => {
			c = t.shortcut ?? s;
			let n = t.continue ?? a, r = t.ignoreKeys ?? o;
			if (u.push(l.length), d.push(e), n && !c) for (let t in e) {
				if (!Object.hasOwn(e, t) || r && r.has(t)) continue;
				let n = e[t];
				n && typeof n == "object" && l.push(n);
			}
		}, m = () => {
			for (; l.length > 0 && l.length < this.maxStackSize;) {
				let e = l.pop();
				if (!c) {
					if (Array.isArray(e)) {
						for (let t = e.length - 1; t >= 0; t--) {
							let n = e[t];
							typeof n == "object" && n && l.push(n);
						}
						let t = f();
						if (t) return t.then(m);
						continue;
					}
					let t = n(e);
					if (r && dg(t)) return t.then((t) => {
						p(e, t);
						let n = f();
						return n ? n.then(m) : m();
					});
					p(e, t);
				}
				let t = f();
				if (t) return t.then(m);
			}
			if (l.length >= this.maxStackSize) throw Error("Transform object stack overflowed");
		};
		return m();
	}
}, pg = class e extends fg {
	defaultNodePreVisitor;
	constructor(e = {}, t = {}) {
		super(e), this.defaultNodePreVisitor = t;
	}
	clone(t = {}, n = {}) {
		return new e({
			...this.defaultContext,
			...t
		}, {
			...this.defaultNodePreVisitor,
			...n
		});
	}
	transformNode(e, t) {
		let n = (e, n) => {
			let r, i = e;
			return i.type && (r = t[i.type]?.transform), r ? r(i, n) : e;
		}, r = this.defaultNodePreVisitor;
		return this.transformObject(e, n, (e) => {
			let n, i = {}, a = e;
			return a.type && (n = t[a.type]?.preVisitor, i = r[a.type] ?? i), n ? {
				...i,
				...n(a)
			} : i;
		});
	}
	transformNodeAsync(e, t) {
		let n = (e, n) => {
			let r, i = e;
			return i.type && (r = t[i.type]?.transform), r ? r(i, n) : e;
		}, r = this.defaultNodePreVisitor;
		return this.transformObjectAsync(e, n, (e) => {
			let n, i = {}, a = e;
			if (a.type && (n = t[a.type]?.preVisitor, i = r[a.type] ?? i), !n) return i;
			let o = n(a);
			return dg(o) ? o.then((e) => ({
				...i,
				...e
			})) : {
				...i,
				...o
			};
		});
	}
	transformNodePreOrder(e, t) {
		let n = this.defaultNodePreVisitor;
		return this.transformObjectPreOrder(e, (e, r) => {
			let i, a = {}, o = e;
			return o.type && (i = t[o.type], a = n[o.type] ?? a), i ? {
				...a,
				...i(e, r)
			} : {
				...a,
				newValue: e,
				reTransform: !1
			};
		});
	}
	transformNodePreOrderAsync(e, t) {
		let n = this.defaultNodePreVisitor;
		return this.transformObjectPreOrderAsync(e, (e, r) => {
			let i, a = {}, o = e;
			if (o.type && (i = t[o.type], a = n[o.type] ?? a), !i) return {
				...a,
				newValue: e,
				reTransform: !1
			};
			let s = i(e, r);
			return dg(s) ? s.then((e) => ({
				...a,
				...e
			})) : {
				...a,
				...s
			};
		});
	}
	visitNode(e, t) {
		let n = (e) => {
			let n = e;
			if (n.type) {
				let e = t[n.type]?.visitor;
				e && e(n);
			}
		}, r = this.defaultNodePreVisitor;
		return this.visitObject(e, n, (e) => {
			let n, i = {}, a = e;
			return a.type && (n = t[a.type]?.preVisitor, i = r[a.type] ?? i), n ? {
				...i,
				...n(a)
			} : i;
		});
	}
	visitNodeAsync(e, t) {
		let n = (e) => {
			let n = e;
			if (n.type) {
				let e = t[n.type]?.visitor;
				if (e) return e(n);
			}
		}, r = this.defaultNodePreVisitor;
		return this.visitObjectAsync(e, n, (e) => {
			let n, i = {}, a = e;
			if (a.type && (n = t[a.type]?.preVisitor, i = r[a.type] ?? i), !n) return i;
			let o = n(a);
			return dg(o) ? o.then((e) => ({
				...i,
				...e
			})) : {
				...i,
				...o
			};
		});
	}
}, mg = class e extends pg {
	constructor(e = {}, t = {}) {
		super(e, t);
	}
	clone(t = {}, n = {}) {
		return new e({
			...this.defaultContext,
			...t
		}, {
			...this.defaultNodePreVisitor,
			...n
		});
	}
	transformNodeSpecific(e, t, n) {
		return this.transformObject(e, (e, r) => {
			let i, a = e;
			if (a.type && a.subType) {
				let e = n[a.type];
				e && (i = e[a.subType]?.transform), i ||= t[a.type]?.transform;
			}
			return i ? i(a, r) : e;
		}, (e) => {
			let r, i = e;
			if (i.type && i.subType) {
				let e = n[i.type];
				e && (r = e[i.subType]?.preVisitor), r ||= t[i.type]?.preVisitor;
			}
			return r ? r(i) : {};
		});
	}
	transformNodeSpecificAsync(e, t, n) {
		return this.transformObjectAsync(e, (e, r) => {
			let i, a = e;
			if (a.type && a.subType) {
				let e = n[a.type];
				e && (i = e[a.subType]?.transform), i ||= t[a.type]?.transform;
			}
			return i ? i(a, r) : e;
		}, (e) => {
			let r, i = e;
			if (i.type && i.subType) {
				let e = n[i.type];
				e && (r = e[i.subType]?.preVisitor), r ||= t[i.type]?.preVisitor;
			}
			return r ? r(i) : {};
		});
	}
	transformNodeSpecificPreOrder(e, t, n) {
		return this.transformObjectPreOrder(e, (e, r) => {
			let i, a = e;
			if (a.type && a.subType) {
				let e = n[a.type];
				e && (i = e[a.subType]), i ||= t[a.type];
			}
			return i ? i(e, r) : { newValue: e };
		});
	}
	transformNodeSpecificPreOrderAsync(e, t, n) {
		return this.transformObjectPreOrderAsync(e, (e, r) => {
			let i, a = e;
			if (a.type && a.subType) {
				let e = n[a.type];
				e && (i = e[a.subType]), i ||= t[a.type];
			}
			return i ? i(e, r) : { newValue: e };
		});
	}
	visitNodeSpecific(e, t, n) {
		this.visitObject(e, (e) => {
			let r, i = e;
			if (i.type && i.subType) {
				let e = n[i.type];
				e && (r = e[i.subType]?.visitor), r ||= t[i.type]?.visitor;
			}
			r && r(i);
		}, (e) => {
			let r, i = e;
			if (i.type && i.subType) {
				let e = n[i.type];
				e && (r = e[i.subType]?.preVisitor), r ||= t[i.type]?.preVisitor;
			}
			return r ? r(i) : {};
		});
	}
	visitNodeSpecificAsync(e, t, n) {
		return this.visitObjectAsync(e, (e) => {
			let r, i = e;
			if (i.type && i.subType) {
				let e = n[i.type];
				e && (r = e[i.subType]?.visitor), r ||= t[i.type]?.visitor;
			}
			if (r) return r(i);
		}, (e) => {
			let r, i = e;
			if (i.type && i.subType) {
				let e = n[i.type];
				e && (r = e[i.subType]?.preVisitor), r ||= t[i.type]?.preVisitor;
			}
			return r ? r(i) : {};
		});
	}
}, H;
(function(e) {
	e.Str = "builtInStr", e.Lang = "builtInLang", e.Langmatches = "builtInLangmatches", e.Datatype = "builtInDatatype", e.Bound = "builtInBound", e.Iri = "builtInIri", e.Uri = "builtInUri", e.Bnode = "builtInBnode", e.Rand = "builtInRand", e.Abs = "builtInAbs", e.Ceil = "builtInCeil", e.Floor = "builtInFloor", e.Round = "builtInRound", e.Concat = "builtInConcat", e.Strlen = "builtInStrlen", e.Ucase = "builtInUcase", e.Lcase = "builtInLcase", e.Encode_for_uri = "builtInEncode_for_uri", e.Contains = "builtInContains", e.Strstarts = "builtInStrstarts", e.Strends = "builtInStrends", e.Strbefore = "builtInStrbefore", e.Strafter = "builtInStrafter", e.Year = "builtInYear", e.Month = "builtInMonth", e.Day = "builtInDay", e.Hours = "builtInHours", e.Minutes = "builtInMinutes", e.Seconds = "builtInSeconds", e.Timezone = "builtInTimezone", e.Tz = "builtInTz", e.Now = "builtInNow", e.Uuid = "builtInUuid", e.Struuid = "builtInStruuid", e.Md5 = "builtInMd5", e.Sha1 = "builtInSha1", e.Sha256 = "builtInSha256", e.Sha384 = "builtInSha384", e.Sha512 = "builtInSha512", e.Coalesce = "builtInCoalesce", e.If = "builtInIf", e.Strlang = "builtInStrlang", e.Strdt = "builtInStrdt", e.Sameterm = "builtInSameterm", e.Isiri = "builtInIsiri", e.Isuri = "builtInIsuri", e.Isblank = "builtInIsblank", e.Isliteral = "builtInIsliteral", e.Isnumeric = "builtInIsnumeric", e.Regex = "builtInRegex", e.Substr = "builtInSubstr", e.Replace = "builtInReplace", e.Exists = "builtInExists", e.Notexists = "builtInNotexists", e.Count = "builtInCount", e.Sum = "builtInSum", e.Min = "builtInMin", e.Max = "builtInMax", e.Avg = "builtInAvg", e.Sample = "builtInSample", e.Group_concat = "builtInGroup_concat";
})(H ||= {});
function U(e) {
	return e.charAt(0).toUpperCase() + e.slice(1);
}
var hg = B({
	name: U(H.Str),
	pattern: /str/i,
	label: "STR"
}), gg = B({
	name: U(H.Lang),
	pattern: /lang/i,
	label: "LANG"
}), _g = B({
	name: U(H.Langmatches),
	pattern: /langmatches/i,
	label: "LANGMATCHES"
}), vg = B({
	name: U(H.Datatype),
	pattern: /datatype/i,
	label: "DATATYPE"
}), yg = B({
	name: U(H.Bound),
	pattern: /bound/i,
	label: "BOUND"
}), bg = B({
	name: U(H.Iri),
	pattern: /iri/i,
	label: "IRI"
}), xg = B({
	name: U(H.Uri),
	pattern: /uri/i,
	label: "URI"
}), Sg = B({
	name: U(H.Bnode),
	pattern: /bnode/i,
	label: "BNODE"
}), Cg = B({
	name: U(H.Rand),
	pattern: /rand/i,
	label: "RAND"
}), wg = B({
	name: U(H.Abs),
	pattern: /abs/i,
	label: "ABS"
}), Tg = B({
	name: U(H.Ceil),
	pattern: /ceil/i,
	label: "CEIL"
}), Eg = B({
	name: U(H.Floor),
	pattern: /floor/i,
	label: "FLOOR"
}), Dg = B({
	name: U(H.Round),
	pattern: /round/i,
	label: "ROUND"
}), Og = B({
	name: U(H.Concat),
	pattern: /concat/i,
	label: "CONCAT"
}), kg = B({
	name: U(H.Strlen),
	pattern: /strlen/i,
	label: "STRLEN"
}), Ag = B({
	name: U(H.Ucase),
	pattern: /ucase/i,
	label: "UCASE"
}), jg = B({
	name: U(H.Lcase),
	pattern: /lcase/i,
	label: "LCASE"
}), Mg = B({
	name: U(H.Encode_for_uri),
	pattern: /encode_for_uri/i,
	label: "ENCODE_FOR_URI"
}), Ng = B({
	name: U(H.Contains),
	pattern: /contains/i,
	label: "CONTAINS"
}), Pg = B({
	name: U(H.Strstarts),
	pattern: /strstarts/i,
	label: "STRSTARTS"
}), Fg = B({
	name: U(H.Strends),
	pattern: /strends/i,
	label: "STRENDS"
}), Ig = B({
	name: U(H.Strbefore),
	pattern: /strbefore/i,
	label: "STRBEFORE"
}), Lg = B({
	name: U(H.Strafter),
	pattern: /strafter/i,
	label: "STRAFTER"
}), Rg = B({
	name: U(H.Year),
	pattern: /year/i,
	label: "YEAR"
}), zg = B({
	name: U(H.Month),
	pattern: /month/i,
	label: "MONTH"
}), Bg = B({
	name: U(H.Day),
	pattern: /day/i,
	label: "DAY"
}), Vg = B({
	name: U(H.Hours),
	pattern: /hours/i,
	label: "HOURS"
}), Hg = B({
	name: U(H.Minutes),
	pattern: /minutes/i,
	label: "MINUTES"
}), Ug = B({
	name: U(H.Seconds),
	pattern: /seconds/i,
	label: "SECONDS"
}), Wg = B({
	name: U(H.Timezone),
	pattern: /timezone/i,
	label: "TIMEZONE"
}), Gg = B({
	name: U(H.Tz),
	pattern: /tz/i,
	label: "TZ"
}), Kg = B({
	name: U(H.Now),
	pattern: /now/i,
	label: "NOW"
}), qg = B({
	name: U(H.Uuid),
	pattern: /uuid/i,
	label: "UUID"
}), Jg = B({
	name: U(H.Struuid),
	pattern: /struuid/i,
	label: "STRUUID"
}), Yg = B({
	name: U(H.Md5),
	pattern: /md5/i,
	label: "MD5"
}), Xg = B({
	name: U(H.Sha1),
	pattern: /sha1/i,
	label: "SHA1"
}), Zg = B({
	name: U(H.Sha256),
	pattern: /sha256/i,
	label: "SHA256"
}), Qg = B({
	name: U(H.Sha384),
	pattern: /sha384/i,
	label: "SHA384"
}), $g = B({
	name: U(H.Sha512),
	pattern: /sha512/i,
	label: "SHA512"
}), e_ = B({
	name: U(H.Coalesce),
	pattern: /coalesce/i,
	label: "COALESCE"
}), t_ = B({
	name: U(H.If),
	pattern: /if/i,
	label: "IF"
}), n_ = B({
	name: U(H.Strlang),
	pattern: /strlang/i,
	label: "STRLANG"
}), r_ = B({
	name: U(H.Strdt),
	pattern: /strdt/i,
	label: "STRDT"
}), i_ = B({
	name: U(H.Sameterm),
	pattern: /sameterm/i,
	label: "SAMETERM"
}), a_ = B({
	name: U(H.Isiri),
	pattern: /isiri/i,
	label: "ISIRI"
}), o_ = B({
	name: U(H.Isuri),
	pattern: /isuri/i,
	label: "ISURI"
}), s_ = B({
	name: U(H.Isblank),
	pattern: /isblank/i,
	label: "ISBLANK"
}), c_ = B({
	name: U(H.Isliteral),
	pattern: /isliteral/i,
	label: "ISLITERAL"
}), l_ = B({
	name: U(H.Isnumeric),
	pattern: /isnumeric/i,
	label: "ISNUMERIC"
}), u_ = B({
	name: U(H.Regex),
	pattern: /regex/i,
	label: "REGEX"
}), d_ = B({
	name: U(H.Substr),
	pattern: /substr/i,
	label: "SUBSTR"
}), f_ = B({
	name: U(H.Replace),
	pattern: /replace/i,
	label: "REPLACE"
}), p_ = B({
	name: U(H.Exists),
	pattern: /exists/i,
	label: "EXISTS"
}), m_ = B({
	name: U(H.Notexists),
	pattern: /not exists/i,
	label: "NOT EXISTS"
}), h_ = B({
	name: U(H.Count),
	pattern: /count/i,
	label: "COUNT"
}), g_ = B({
	name: U(H.Sum),
	pattern: /sum/i,
	label: "SUM"
}), __ = B({
	name: U(H.Min),
	pattern: /min/i,
	label: "MIN"
}), v_ = B({
	name: U(H.Max),
	pattern: /max/i,
	label: "MAX"
}), y_ = B({
	name: U(H.Avg),
	pattern: /avg/i,
	label: "AVG"
}), b_ = B({
	name: U(H.Sample),
	pattern: /sample/i,
	label: "SAMPLE"
}), x_ = B({
	name: U(H.Group_concat),
	pattern: /group_concat/i,
	label: "GROUP_CONCAT"
}), S_ = sg.create().add(_g, vg, gg, yg, bg, xg, Sg, Cg, wg, Tg, Eg, Dg, Og, kg, Ag, jg, Mg, Ng, Pg, Fg, Ig, Lg, Rg, zg, Bg, Vg, Hg, Ug, Wg, Gg, Kg, qg, Jg, Yg, Xg, Zg, Qg, $g, e_, t_, n_, r_, i_, a_, o_, s_, c_, l_, u_, d_, f_, p_, m_, h_, g_, __, v_, y_, b_, x_, hg), C_ = B({
	name: "NamedGraph",
	pattern: /named/i,
	label: "NAMED"
}), w_ = B({
	name: "DefaultGraph",
	pattern: /default/i,
	label: "DEFAULT"
}), T_ = B({
	name: "Graph",
	pattern: /graph/i,
	label: "GRAPH"
}), E_ = B({
	name: "GraphAll",
	pattern: /all/i,
	label: "ALL"
}), D_ = sg.create().add(C_, w_, T_, E_), O_ = /[A-Za-z\u00C0-\u00D6\u00D8-\u00F6\u00F8-\u02FF\u0370-\u037D\u037F-\u1FFF\u200C\u200D\u2070-\u218F\u2C00-\u2FEF\u3001-\uD7FF\uF900-\uFDCF\uFDF0-\uFFFD]|[\uD800-\uDB7F][\uDC00-\uDFFF]/, k_ = RegExp(`(${O_.source})|_`), A_ = RegExp(`((${k_.source})|[0-9])((${k_.source})|[0-9]|[\u00B7\u0300-\u036F\u203F-\u2040])*`), j_ = /<([^\u0000-\u0020"<>\\^`{|}])*>/, M_ = RegExp(`(${k_.source})|[\\-0-9\u00B7\u0300-\u036F\u203F-\u2040]`), N_ = RegExp(`(${O_.source})(((${M_.source})|\\.)*(${M_.source}))?`), P_ = RegExp(`(${N_.source})?:`), F_ = RegExp("(%[\\dA-Fa-f]{2})|(\\\\[!#$%&'()*+,./;=?@_~-])"), I_ = RegExp(`((${k_.source})|:|[0-9]|(${F_.source}))(((${M_.source})|\\.|:|(${F_.source}))*((${M_.source})|:|(${F_.source})))?`), L_ = RegExp(`(${P_.source})(${I_.source})`), R_ = RegExp(`_:((${k_.source})|[0-9])(((${M_.source})|\\.)*(${M_.source}))?`), z_ = RegExp(`\\?(${A_.source})`), B_ = RegExp(`\\$(${A_.source})`), V_ = /@[A-Za-z]+(-[\dA-Za-z]+)*/, H_ = /\d+/, U_ = /\d+\.\d+/, W_ = /[Ee][+-]?\d+/, G_ = RegExp(`([0-9]+\\.[0-9]*(${W_.source}))|(\\.[0-9]+(${W_.source}))|([0-9]+(${W_.source}))`), K_ = RegExp(`\\+(${H_.source})`), q_ = RegExp(`\\+(${U_.source})`), J_ = RegExp(`\\+(${G_.source})`), Y_ = RegExp(`-(${H_.source})`), X_ = RegExp(`-(${U_.source})`), Z_ = RegExp(`-(${G_.source})`), Q_ = /\\["'\\bfnrt]/, $_ = RegExp(`'(([^\\u0027\\u005C\\u000A\u000D])|(${Q_.source}))*'`), ev = RegExp(`"(([^\\u0022\\u005C\\u000A\\u000D])|(${Q_.source}))*"`), tv = RegExp(`'''(('|(''))?([^'\\\\]|(${Q_.source})))*'''`), nv = RegExp(`"""(("|(""))?([^"\\\\]|(${Q_.source})))*"""`), rv = /[\u0009\u000A\u000D ]/, iv = RegExp(`\\((${rv.source})*\\)`), av = RegExp(`\\[(${rv.source})*\\]`), ov = /#[^\n]*/, sv = RegExp(`(((${rv.source})+)|((${ov.source})\n))+`), cv = B({
	name: "LCurly",
	pattern: "{",
	label: "{"
}), lv = B({
	name: "RCurly",
	pattern: "}",
	label: "}"
}), uv = B({
	name: "Dot",
	pattern: ".",
	label: "."
}), dv = B({
	name: "Comma",
	pattern: ",",
	label: ","
}), fv = B({
	name: "Semi",
	pattern: ";",
	label: ";"
}), W = B({
	name: "LParen",
	pattern: "(",
	label: "("
}), G = B({
	name: "RParen",
	pattern: ")",
	label: ")"
}), pv = B({
	name: "LSquare",
	pattern: "[",
	label: "["
}), mv = B({
	name: "RSquare",
	pattern: "]",
	label: "]"
}), hv = B({
	name: "Pipe",
	pattern: "|",
	label: "|"
}), gv = B({
	name: "Slash",
	pattern: "/",
	label: "/"
}), _v = B({
	name: "Hat",
	pattern: "^",
	label: "^"
}), vv = B({
	name: "Question",
	pattern: "?",
	label: "?"
}), yv = B({
	name: "Star",
	pattern: "*",
	label: "*"
}), bv = B({
	name: "OpPlus",
	pattern: "+",
	label: "+"
}), xv = B({
	name: "OpMinus",
	pattern: "-",
	label: "-"
}), Sv = B({
	name: "Exclamation",
	pattern: "!",
	label: "!"
}), Cv = B({
	name: "LogicAnd",
	pattern: "&&",
	label: "&&"
}), wv = B({
	name: "LogicOr",
	pattern: "||",
	label: "||"
}), Tv = B({
	name: "Equal",
	pattern: "=",
	label: "="
}), Ev = B({
	name: "NotEqual",
	pattern: "!=",
	label: "!="
}), Dv = B({
	name: "LessThan",
	pattern: "<",
	label: "<"
}), Ov = B({
	name: "GreaterThan",
	pattern: ">",
	label: ">"
}), kv = B({
	name: "LessThanEqual",
	pattern: "<=",
	label: "<="
}), Av = B({
	name: "GreaterThanEqual",
	pattern: ">=",
	label: ">="
}), jv = B({
	name: "Hathat",
	pattern: "^^",
	label: "^^"
}), Mv = sg.create().add(Cv, wv, Ev, kv, Av, cv, lv, uv, dv, fv, W, G, pv, mv, hv, gv, jv, _v, vv, yv, bv, xv, Sv, Tv, Dv, Ov), Nv = B({
	name: "IriRef",
	pattern: j_
}), Pv = B({
	name: "PNameLn",
	pattern: L_
}), Fv = B({
	name: "PNameNs",
	pattern: P_,
	longer_alt: [Pv]
}), Iv = B({
	name: "BlankNodeLabel",
	pattern: R_
}), Lv = B({
	name: "Var1",
	pattern: z_
}), Rv = B({
	name: "Var2",
	pattern: B_
}), zv = B({
	name: "LangTag",
	pattern: V_
}), Bv = B({
	name: "Integer",
	pattern: H_
}), Vv = B({
	name: "Decimal",
	pattern: U_
}), Hv = B({
	name: "Double",
	pattern: G_
}), Uv = B({
	name: "IntegerPositive",
	pattern: K_
}), Wv = B({
	name: "DecimalPositive",
	pattern: q_
}), Gv = B({
	name: "DoublePositive",
	pattern: J_
}), Kv = B({
	name: "IntegerNegative",
	pattern: Y_
}), qv = B({
	name: "DecimalNegative",
	pattern: X_
}), Jv = B({
	name: "DoubleNegative",
	pattern: Z_
}), Yv = B({
	name: "StringLiteral1",
	pattern: $_
}), Xv = B({
	name: "StringLiteral2",
	pattern: ev
}), Zv = B({
	name: "StringLiteralLong1",
	pattern: tv
}), Qv = B({
	name: "StringLiteralLong2",
	pattern: nv
}), $v = B({
	name: "Ws",
	pattern: rv,
	group: L.SKIPPED
}), ey = B({
	name: "Comment",
	pattern: ov,
	group: L.SKIPPED
}), ty = B({
	name: "Nil",
	pattern: iv
}), ny = B({
	name: "Anon",
	pattern: av
}), ry = sg.create().add(Nv, Fv, Pv, Iv, Lv, Rv, zv, Hv, Vv, Bv, Gv, Wv, Uv, Jv, qv, Kv, Zv, Qv, Yv, Xv, $v, ey, ty, ny), iy = B({
	name: "BaseDecl",
	pattern: /base/i,
	label: "BASE"
}), ay = B({
	name: "PrefixDecl",
	pattern: /prefix/i,
	label: "PREFIX"
}), oy = B({
	name: "Select",
	pattern: /select/i,
	label: "SELECT"
}), sy = B({
	name: "Distinct",
	pattern: /distinct/i,
	label: "DISTINCT"
}), cy = B({
	name: "Reduced",
	pattern: /reduced/i,
	label: "REDUCED"
}), ly = B({
	name: "As",
	pattern: /as/i,
	label: "AS"
}), uy = B({
	name: "Construct",
	pattern: /construct/i,
	label: "CONSTRUCT"
}), dy = B({
	name: "Describe",
	pattern: /describe/i,
	label: "DESCRIBE"
}), fy = B({
	name: "Ask",
	pattern: /ask/i,
	label: "ASK"
}), py = B({
	name: "From",
	pattern: /from/i,
	label: "FROM"
}), my = B({
	name: "Where",
	pattern: /where/i,
	label: "WHERE"
}), hy = B({
	name: "GroupByGroup",
	pattern: /group/i,
	label: "_GROUP_ BY"
}), gy = B({
	name: "By",
	pattern: /by/i,
	label: "BY"
}), _y = B({
	name: "Having",
	pattern: /having/i,
	label: "HAVING"
}), vy = B({
	name: "Order",
	pattern: /order/i,
	label: "_ORDER_ BY"
}), yy = B({
	name: "OrderAsc",
	pattern: /asc/i,
	label: "ASC"
}), by = B({
	name: "OrderDesc",
	pattern: /desc/i,
	label: "DESC"
}), xy = B({
	name: "Limit",
	pattern: /limit/i,
	label: "LIMIT"
}), Sy = B({
	name: "Offset",
	pattern: /offset/i,
	label: "OFFSET"
}), Cy = B({
	name: "Values",
	pattern: /values/i,
	label: "VALUES"
}), wy = B({
	name: "Load",
	pattern: /load/i,
	label: "LOAD"
}), Ty = B({
	name: "Silent",
	pattern: /silent/i,
	label: "SILENT"
}), Ey = B({
	name: "LoadInto",
	pattern: /into/i,
	label: "INTO"
}), Dy = B({
	name: "Clear",
	pattern: /clear/i,
	label: "CLEAR"
}), Oy = B({
	name: "Drop",
	pattern: /drop/i,
	label: "DROP"
}), ky = B({
	name: "Create",
	pattern: /create/i,
	label: "CREATE"
}), Ay = B({
	name: "Add",
	pattern: /add/i,
	label: "ADD"
}), jy = B({
	name: "To",
	pattern: /to/i,
	label: "TO"
}), My = B({
	name: "Move",
	pattern: /move/i,
	label: "MOVE"
}), Ny = B({
	name: "Copy",
	pattern: /copy/i,
	label: "COPY"
}), Py = B({
	name: "ModifyWith",
	pattern: /with/i,
	label: "WITH"
}), Fy = B({
	name: "DeleteDataClause",
	pattern: RegExp(`delete(${sv.source})data`, "i"),
	label: "DELETE DATA"
}), Iy = B({
	name: "DeleteWhereClause",
	pattern: RegExp(`delete(${sv.source})where`, "i"),
	label: "DELETE WHERE"
}), Ly = B({
	name: "DeleteClause",
	pattern: /delete/i,
	label: "DELETE"
}), Ry = B({
	name: "InsertDataClause",
	pattern: RegExp(`insert(${sv.source})data`, "i"),
	label: "INSERT DATA"
}), zy = B({
	name: "InsertClause",
	pattern: /insert/i,
	label: "insert"
}), By = B({
	name: "UsingClause",
	pattern: /using/i,
	label: "USING"
}), Vy = B({
	name: "Optional",
	pattern: /optional/i,
	label: "OPTIONAL"
}), Hy = B({
	name: "Service",
	pattern: /service/i,
	label: "SERVICE"
}), Uy = B({
	name: "Bind",
	pattern: /bind/i,
	label: "BIND"
}), Wy = B({
	name: "Undef",
	pattern: /undef/i,
	label: "UNDEF"
}), Gy = B({
	name: "Minus",
	pattern: /minus/i,
	label: "MINUS"
}), Ky = B({
	name: "Union",
	pattern: /union/i,
	label: "UNION"
}), qy = B({
	name: "Filter",
	pattern: /filter/i,
	label: "FILTER"
}), Jy = B({
	name: "a",
	pattern: "a",
	label: "type declaration 'a'"
}), Yy = B({
	name: "True",
	pattern: /true/i,
	label: "true"
}), Xy = B({
	name: "False",
	pattern: /false/i,
	label: "false"
}), Zy = B({
	name: "In",
	pattern: /in/i,
	label: "IN"
}), Qy = B({
	name: "NotIn",
	pattern: /not[\u0020\u0009\u000D\u000A]+in/i,
	label: "NOT IN"
}), $y = B({
	name: "Separator",
	pattern: /separator/i,
	label: "SEPARATOR"
}), eb = sg.create().add(iy, ay, oy, sy, cy, uy, dy, fy, py, my, _y, hy, gy, vy, yy, by, xy, Sy, Cy, wy, Ty, Ey, Dy, Oy, ky, Ay, jy, My, Ny, Py, Iy, Fy, Ly, Ry, zy, By, Vy, Hy, Uy, Wy, Gy, Ky, qy, ly, Jy, Yy, Xy, Zy, Qy, $y), tb = sg.create(ry).merge(eb).merge(S_).merge(D_).merge(Mv).moveAfter(y_, Jy).moveBefore(Jy, E_).moveAfter(x_, hy), nb = "contextDef";
function rb(e) {
	return class extends e {
		contextDefinitionPrefix(e, t, n) {
			return {
				type: nb,
				subType: "prefix",
				key: t,
				value: n,
				loc: e
			};
		}
		isContextDefinitionPrefix(e) {
			return this.isOfSubType(e, nb, "prefix");
		}
		contextDefinitionBase(e, t) {
			return {
				type: "contextDef",
				subType: "base",
				value: t,
				loc: e
			};
		}
		isContextDefinitionBase(e) {
			return this.isOfSubType(e, nb, "base");
		}
	};
}
//#endregion
//#region ../../wt-query-graph/web/node_modules/@traqula/rules-sparql-1-1/dist/esm/lib/factoryMixins/ExpressionFactory.js
var ib = "expression";
function ab(e) {
	return class extends e {
		isExpressionPure(e) {
			return this.isOfType(e, ib);
		}
		formatOperator(e) {
			return e.toLowerCase().replaceAll(" ", "");
		}
		expressionOperation(e, t, n) {
			return {
				type: ib,
				subType: "operation",
				operator: this.formatOperator(e),
				args: t,
				loc: n
			};
		}
		isExpressionOperator(e) {
			return this.isOfSubType(e, ib, "operation");
		}
		expressionFunctionCall(e, t, n, r) {
			return {
				type: "expression",
				subType: "functionCall",
				function: e,
				args: t,
				distinct: n,
				loc: r
			};
		}
		isExpressionFunctionCall(e) {
			return this.isOfSubType(e, ib, "functionCall");
		}
		expressionPatternOperation(e, t, n) {
			return {
				type: ib,
				subType: "patternOperation",
				operator: this.formatOperator(e),
				args: t,
				loc: n
			};
		}
		isExpressionPatternOperation(e) {
			return this.isOfSubType(e, ib, "patternOperation");
		}
		aggregate(e, t, n, r, i) {
			let a = {
				type: "expression",
				subType: "aggregate",
				aggregation: this.formatOperator(e),
				distinct: t,
				loc: i
			};
			return this.isOfType(n, "wildcard") || r === void 0 ? {
				...a,
				expression: [n]
			} : {
				...a,
				expression: [n],
				separator: r
			};
		}
		isExpressionAggregate(e) {
			return this.isOfSubType(e, ib, "aggregate");
		}
		isExpressionAggregateSeparator(e) {
			return this.isOfSubType(e, ib, "aggregate") && typeof e.separator == "string";
		}
		isExpressionAggregateOnWildcard(e) {
			let t = e;
			return this.isOfSubType(e, ib, "aggregate") && Array.isArray(t.expression) && t.expression.length === 1 && this.isOfType(t.expression[0], "wildcard");
		}
		isExpressionAggregateDefault(e) {
			let t = e;
			return this.isOfSubType(e, ib, "operation") && Array.isArray(t.expression) && t.expression.length === 1 && !this.isOfType(t.expression[0], "wildcard");
		}
	};
}
//#endregion
//#region ../../wt-query-graph/web/node_modules/@traqula/rules-sparql-1-1/dist/esm/lib/factoryMixins/GraphRefFactory.js
var ob = "graphRef";
function sb(e) {
	return class extends e {
		isGraphRef(e) {
			return this.isOfType(e, ob);
		}
		graphRefDefault(e) {
			return {
				type: ob,
				subType: "default",
				loc: e
			};
		}
		isGraphRefDefault(e) {
			return this.isOfSubType(e, ob, "default");
		}
		graphRefNamed(e) {
			return {
				type: ob,
				subType: "named",
				loc: e
			};
		}
		isGraphRefNamed(e) {
			return this.isOfSubType(e, ob, "named");
		}
		graphRefAll(e) {
			return {
				type: ob,
				subType: "all",
				loc: e
			};
		}
		isGraphRefAll(e) {
			return this.isOfSubType(e, ob, "all");
		}
		graphRefSpecific(e, t) {
			return {
				type: ob,
				subType: "specific",
				graph: e,
				loc: t
			};
		}
		isGraphRefSpecific(e) {
			return this.isOfSubType(e, ob, "specific");
		}
	};
}
//#endregion
//#region ../../wt-query-graph/web/node_modules/@traqula/rules-sparql-1-1/dist/esm/lib/factoryMixins/mixins.js
function cb(e) {
	return new lb(e);
}
var lb = class {
	input;
	constructor(e) {
		this.input = e;
	}
	call(e) {
		return this.input = e(this.input), this;
	}
	returns() {
		return this.input;
	}
}, ub = "path";
function db(e) {
	return class extends e {
		isPathPure(e) {
			return this.isOfType(e, ub);
		}
		path(e, t, n) {
			let r = {
				type: ub,
				loc: n,
				items: t
			};
			if (e === "|" || e === "/") return {
				...r,
				subType: e
			};
			if ((e === "?" || e === "*" || e === "+" || e === "^" && this.isPathPure(t[0])) && t.length === 1 || e === "^" && t.length === 1 && !this.isPathPure(t[0]) || e === "!" && t.length === 1 && (this.isPathAlternativeLimited(t[0]) || !this.isPathPure(t[0]) || this.isPathNegatedElt(t[0]))) return {
				...r,
				subType: e,
				items: t
			};
			throw Error("Invalid path type");
		}
		isPathOfType(e, t) {
			return this.isOfType(e, ub) && t.includes(e.subType);
		}
		isPathChain(e) {
			return this.isOfSubType(e, ub, "/") || this.isOfSubType(e, ub, "|");
		}
		isPathModified(e) {
			return this.isOfSubType(e, ub, "?") || this.isOfSubType(e, ub, "*") || this.isOfSubType(e, ub, "+") || this.isOfSubType(e, ub, "^");
		}
		isPathNegatedElt(e) {
			let t = e;
			return this.isOfSubType(e, ub, "^") && Array.isArray(t.items) && t.items.length === 1 && t.items[0] !== null && typeof t.items[0] == "object" && !this.isPathPure(t.items[0]);
		}
		isPathNegated(e) {
			return this.isOfSubType(e, ub, "!");
		}
		isPathAlternativeLimited(e) {
			let t = e;
			return this.isOfSubType(e, ub, "|") && Array.isArray(t.items) && t.items.every((e) => !this.isPathPure(e) || this.isPathNegatedElt(e));
		}
	};
}
//#endregion
//#region ../../wt-query-graph/web/node_modules/@traqula/rules-sparql-1-1/dist/esm/lib/factoryMixins/Patternfactory.js
var K = "pattern";
function fb(e) {
	return class extends e {
		isPattern(e) {
			return this.isOfType(e, K);
		}
		patternBgp(e, t) {
			return {
				type: K,
				subType: "bgp",
				triples: e,
				loc: t
			};
		}
		isPatternBgp(e) {
			return this.isOfSubType(e, K, "bgp");
		}
		patternGroup(e, t) {
			return {
				type: K,
				subType: "group",
				patterns: e,
				loc: t
			};
		}
		isPatternGroup(e) {
			return this.isOfSubType(e, K, "group");
		}
		patternGraph(e, t, n) {
			return {
				type: K,
				subType: "graph",
				name: e,
				patterns: t,
				loc: n
			};
		}
		isPatternGraph(e) {
			return this.isOfSubType(e, K, "graph");
		}
		patternOptional(e, t) {
			return {
				type: K,
				subType: "optional",
				patterns: e,
				loc: t
			};
		}
		isPatternOptional(e) {
			return this.isOfSubType(e, K, "optional");
		}
		patternValues(e, t, n) {
			return {
				type: K,
				subType: "values",
				variables: e,
				values: t,
				loc: n
			};
		}
		isPatternValues(e) {
			return this.isOfSubType(e, K, "values");
		}
		patternFilter(e, t) {
			return {
				type: K,
				subType: "filter",
				expression: e,
				loc: t
			};
		}
		isPatternFilter(e) {
			return this.isOfSubType(e, K, "filter");
		}
		patternBind(e, t, n) {
			return {
				type: K,
				subType: "bind",
				expression: e,
				variable: t,
				loc: n
			};
		}
		isPatternBind(e) {
			return this.isOfSubType(e, K, "bind");
		}
		patternUnion(e, t) {
			return {
				type: K,
				subType: "union",
				patterns: e,
				loc: t
			};
		}
		isPatternUnion(e) {
			return this.isOfSubType(e, K, "union");
		}
		patternMinus(e, t) {
			return {
				type: K,
				subType: "minus",
				patterns: e,
				loc: t
			};
		}
		isPatternMinus(e) {
			return this.isOfSubType(e, K, "minus");
		}
		patternService(e, t, n, r) {
			return {
				type: K,
				subType: "service",
				silent: n,
				name: e,
				patterns: t,
				loc: r
			};
		}
		isPatternService(e) {
			return this.isOfSubType(e, K, "service");
		}
	};
}
//#endregion
//#region ../../wt-query-graph/web/node_modules/@traqula/rules-sparql-1-1/dist/esm/lib/factoryMixins/QueryFactory.js
var pb = "query";
function mb(e) {
	return class extends e {
		isQuery(e) {
			return this.isOfType(e, pb);
		}
		isQuerySelect(e) {
			return this.isOfSubType(e, pb, "select");
		}
		queryConstruct(e, t, n, r, i, a, o) {
			return {
				type: "query",
				subType: "construct",
				context: t,
				template: n,
				where: r,
				solutionModifiers: i,
				datasets: a,
				values: o,
				loc: e
			};
		}
		isQueryConstruct(e) {
			return this.isOfSubType(e, pb, "construct");
		}
		isQueryDescribe(e) {
			return this.isOfSubType(e, pb, "describe");
		}
		isQueryAsk(e) {
			return this.isOfSubType(e, pb, "ask");
		}
		querySelect(e, t) {
			return {
				type: pb,
				subType: "select",
				...e,
				loc: t
			};
		}
	};
}
//#endregion
//#region ../../wt-query-graph/web/node_modules/@traqula/rules-sparql-1-1/dist/esm/lib/factoryMixins/SolutionModifiersFactory.js
var hb = "solutionModifier";
function gb(e) {
	return class extends e {
		isSolutionModifier(e) {
			return this.isOfType(e, hb);
		}
		solutionModifierHaving(e, t) {
			return {
				type: hb,
				subType: "having",
				having: e,
				loc: t
			};
		}
		isSolutionModifierHaving(e) {
			return this.isOfSubType(e, hb, "having");
		}
		solutionModifierOrder(e, t) {
			return {
				type: hb,
				subType: "order",
				orderDefs: e,
				loc: t
			};
		}
		isSolutionModifierOrder(e) {
			return this.isOfSubType(e, hb, "order");
		}
		solutionModifierLimitOffset(e, t, n) {
			return {
				type: hb,
				subType: "limitOffset",
				limit: e,
				offset: t,
				loc: n
			};
		}
		isSolutionModifierLimitOffset(e) {
			return this.isOfSubType(e, hb, "limitOffset");
		}
		solutionModifierGroup(e, t) {
			return {
				type: "solutionModifier",
				subType: "group",
				groupings: e,
				loc: t
			};
		}
		isSolutionModifierGroup(e) {
			return this.isOfSubType(e, hb, "group");
		}
	};
}
//#endregion
//#region ../../wt-query-graph/web/node_modules/@traqula/rules-sparql-1-1/dist/esm/lib/factoryMixins/TermFactory.js
var _b = "term";
function vb(e) {
	return class extends e {
		__blankNodeCounter = 0;
		resetBlankNodeCounter() {
			this.__blankNodeCounter = 0;
		}
		isTerm(e) {
			return this.isOfType(e, "term");
		}
		termBlank(e, t) {
			let n = {
				type: "term",
				subType: "blankNode",
				loc: t
			};
			return e === void 0 ? {
				...n,
				label: `g_${this.__blankNodeCounter++}`
			} : {
				...n,
				label: `e_${e}`
			};
		}
		isTermBlank(e) {
			return this.isOfSubType(e, _b, "blankNode");
		}
		termLiteral(e, t, n) {
			return {
				type: _b,
				subType: "literal",
				value: t,
				langOrIri: n,
				loc: e
			};
		}
		isTermLiteral(e) {
			return this.isOfSubType(e, _b, "literal");
		}
		isTermLiteralLangStr(e) {
			return this.isTermLiteral(e) && typeof e.langOrIri == "string";
		}
		isTermLiteralStr(e) {
			return this.isTermLiteral(e) && e.langOrIri === void 0;
		}
		isTermLiteralTyped(e) {
			let t = e;
			return this.isTermLiteral(e) && typeof t.langOrIri == "object" && t.langOrIri !== null && this.isTermNamed(t.langOrIri);
		}
		termVariable(e, t) {
			return {
				type: _b,
				subType: "variable",
				value: e,
				loc: t
			};
		}
		isTermVariable(e) {
			return this.isOfSubType(e, _b, "variable");
		}
		termNamed(e, t, n) {
			let r = {
				type: _b,
				subType: "namedNode",
				value: t,
				loc: e
			};
			return n === void 0 ? r : {
				...r,
				prefix: n
			};
		}
		isTermNamed(e) {
			return this.isOfSubType(e, _b, "namedNode");
		}
		isTermNamedPrefixed(e) {
			let t = e;
			return this.isTermNamed(e) && typeof t.prefix == "string";
		}
	};
}
//#endregion
//#region ../../wt-query-graph/web/node_modules/@traqula/rules-sparql-1-1/dist/esm/lib/factoryMixins/UpdateOperationFactory.js
var yb = "updateOperation";
function bb(e) {
	return class extends e {
		isUpdateOperation(e) {
			return this.isOfType(e, yb);
		}
		updateOperationLoad(e, t, n, r) {
			return {
				type: yb,
				subType: "load",
				silent: n,
				source: t,
				...r && { destination: r },
				loc: e
			};
		}
		isUpdateOperationLoad(e) {
			return this.isOfSubType(e, yb, "load");
		}
		updateOperationClearDrop(e, t, n, r) {
			return {
				type: "updateOperation",
				subType: e,
				silent: t,
				destination: n,
				loc: r
			};
		}
		updateOperationClear(e, t, n) {
			return this.updateOperationClearDrop("clear", t, e, n);
		}
		isUpdateOperationClear(e) {
			return this.isOfSubType(e, yb, "clear");
		}
		updateOperationDrop(e, t, n) {
			return this.updateOperationClearDrop("drop", t, e, n);
		}
		isUpdateOperationDrop(e) {
			return this.isOfSubType(e, yb, "drop");
		}
		updateOperationCreate(e, t, n) {
			return {
				type: "updateOperation",
				subType: "create",
				silent: t,
				destination: e,
				loc: n
			};
		}
		isUpdateOperationCreate(e) {
			return this.isOfSubType(e, yb, "create");
		}
		updateOperationAddMoveCopy(e, t, n, r, i) {
			return {
				type: "updateOperation",
				subType: e,
				silent: r,
				source: t,
				destination: n,
				loc: i
			};
		}
		updateOperationAdd(e, t, n, r) {
			return this.updateOperationAddMoveCopy("add", e, t, n, r);
		}
		isUpdateOperationAdd(e) {
			return this.isOfSubType(e, yb, "add");
		}
		updateOperationMove(e, t, n, r) {
			return this.updateOperationAddMoveCopy("move", e, t, n, r);
		}
		isUpdateOperationMove(e) {
			return this.isOfSubType(e, yb, "move");
		}
		updateOperationCopy(e, t, n, r) {
			return this.updateOperationAddMoveCopy("copy", e, t, n, r);
		}
		isUpdateOperationCopy(e) {
			return this.isOfSubType(e, yb, "copy");
		}
		updateOperationInsDelDataWhere(e, t, n) {
			return {
				type: "updateOperation",
				subType: e,
				data: t,
				loc: n
			};
		}
		updateOperationInsertData(e, t) {
			return this.updateOperationInsDelDataWhere("insertdata", e, t);
		}
		isUpdateOperationInsertData(e) {
			return this.isOfSubType(e, yb, "insertdata");
		}
		updateOperationDeleteData(e, t) {
			return this.updateOperationInsDelDataWhere("deletedata", e, t);
		}
		isUpdateOperationDeleteData(e) {
			return this.isOfSubType(e, yb, "deletedata");
		}
		updateOperationDeleteWhere(e, t) {
			return this.updateOperationInsDelDataWhere("deletewhere", e, t);
		}
		isUpdateOperationDeleteWhere(e) {
			return this.isOfSubType(e, yb, "deletewhere");
		}
		updateOperationModify(e, t, n, r, i, a) {
			return {
				type: "updateOperation",
				subType: "modify",
				insert: t ?? [],
				delete: n ?? [],
				graph: a,
				where: r,
				from: i,
				loc: e
			};
		}
		isUpdateOperationModify(e) {
			return this.isOfSubType(e, yb, "modify");
		}
	};
}
//#endregion
//#region ../../wt-query-graph/web/node_modules/@traqula/rules-sparql-1-1/dist/esm/lib/astFactory.js
var xb = class extends cb(e).call(rb).call(ab).call(sb).call(db).call(fb).call(mb).call(gb).call(vb).call(bb).returns() {
	alwaysSparql11(e) {
		return !0;
	}
	isPath(e) {
		return this.isPathPure(e) || this.isTermNamed(e);
	}
	isExpression(e) {
		return this.isExpressionPure(e) || this.isTermNamed(e) || this.isTermVariable(e) || this.isTermLiteral(e);
	}
	graphNodeIdentifier(e) {
		return e.type === "tripleCollection" ? e.identifier : e;
	}
	triple(e, t, n, r) {
		return {
			type: "triple",
			subject: e,
			predicate: t,
			object: n,
			loc: r ?? this.sourceLocation(e, t, n)
		};
	}
	isTriple(e) {
		return this.isOfType(e, "triple");
	}
	datasetClauses(e, t) {
		return {
			type: "datasetClauses",
			clauses: e,
			loc: t
		};
	}
	isDatasetClauses(e) {
		return this.isOfType(e, "datasetClauses");
	}
	wildcard(e) {
		return {
			type: "wildcard",
			loc: e
		};
	}
	isWildcard(e) {
		return this.isOfType(e, "wildcard");
	}
	isTripleCollection(e) {
		return this.isOfType(e, "tripleCollection");
	}
	tripleCollectionBlankNodeProperties(e, t, n) {
		return {
			type: "tripleCollection",
			subType: "blankNodeProperties",
			identifier: e,
			triples: t,
			loc: n
		};
	}
	isTripleCollectionBlankNodeProperties(e) {
		return this.isOfSubType(e, "tripleCollection", "blankNodeProperties");
	}
	tripleCollectionList(e, t, n) {
		return {
			type: "tripleCollection",
			subType: "list",
			identifier: e,
			triples: t,
			loc: n
		};
	}
	isTripleCollectionList(e) {
		return this.isOfSubType(e, "tripleCollection", "list");
	}
	graphQuads(e, t, n) {
		return {
			type: "graph",
			graph: e,
			triples: t,
			loc: n
		};
	}
	isGraphQuads(e) {
		return super.isOfType(e, "graph");
	}
	isUpdate(e) {
		return super.isOfType(e, "update");
	}
};
//#endregion
//#region ../../wt-query-graph/web/node_modules/@traqula/rules-sparql-1-1/dist/esm/lib/utils.js
function Sb(e) {
	let t = e.replaceAll(/\\u([0-9a-fA-F]{4})|\\U([0-9a-fA-F]{8})/gu, (e, t, n) => {
		if (t) {
			let e = Number.parseInt(t, 16);
			return String.fromCodePoint(e);
		}
		let r = Number.parseInt(n, 16);
		if (r < 65535) return String.fromCodePoint(r);
		let i = r - 65536;
		return String.fromCodePoint(55296 + (i >> 10), 56320 + (i & 1023));
	});
	if (/[\uD800-\uDBFF](?:[^\uDC00-\uDFFF]|$)/u.test(t)) throw Error("Invalid unicode codepoint of surrogate pair without corresponding codepoint");
	return t;
}
var q;
(function(e) {
	e.BOOLEAN = "http://www.w3.org/2001/XMLSchema#boolean", e.INTEGER = "http://www.w3.org/2001/XMLSchema#integer", e.DECIMAL = "http://www.w3.org/2001/XMLSchema#decimal", e.DOUBLE = "http://www.w3.org/2001/XMLSchema#double", e.STRING = "http://www.w3.org/2001/XMLSchema#string", e.FIRST = "http://www.w3.org/1999/02/22-rdf-syntax-ns#first", e.REST = "http://www.w3.org/1999/02/22-rdf-syntax-ns#rest", e.NIL = "http://www.w3.org/1999/02/22-rdf-syntax-ns#nil", e.TYPE = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
})(q ||= {});
var Cb = class extends mg {}, J = new xb(), wb = new Cb();
function Tb(e) {
	if (J.isExpressionAggregate(e)) return [e];
	if (J.isExpressionOperator(e)) {
		let t = [];
		for (let n of e.args) t.push(...Tb(n));
		return t;
	}
	return [];
}
function Eb(e) {
	return J.isTerm(e) && J.isTermVariable(e) ? e.value : J.isExpression(e) ? J.isExpressionAggregate(e) && J.isTermVariable(e.expression[0]) ? e.expression[0].value : void 0 : e.variable.value;
}
function Db(e, t) {
	if (J.isExpressionOperator(e)) for (let n of e.args) Db(n, t);
	else J.isTerm(e) && J.isTermVariable(e) && t.add(e.value);
}
function Ob(e) {
	if (e.variables.length === 1 && J.isWildcard(e.variables[0])) {
		if (e.solutionModifiers.group !== void 0) throw Error("GROUP BY not allowed with wildcard");
		return;
	}
	let t = e.variables, n = t.flatMap((e) => J.isTerm(e) ? [] : Tb(e.expression)).some((e) => e.aggregation === "count" && !e.expression.some((e) => J.isWildcard(e))), r = e.solutionModifiers.group;
	if (n || r) {
		for (let e of t) if (J.isTerm(e)) {
			if (!r || !r.groupings.map((e) => Eb(e)).includes(Eb(e))) throw Error("Variable not allowed in projection");
		} else if (Tb(e.expression).length === 0) {
			let t = /* @__PURE__ */ new Set();
			Db(e.expression, t);
			for (let e of t) if (!r || !r.groupings.map((e) => Eb(e)).includes(e)) throw Error(`Use of ungrouped variable in projection of operation (?${e})`);
		}
	}
	let i = e.where.patterns.filter((e) => e.type === "query");
	if (i.length > 0) {
		let e = /* @__PURE__ */ new Set();
		for (let n of t) "variable" in n && e.add(n.variable.value);
		let n = i.flatMap((e) => e.variables).map((e) => J.isTerm(e) ? e.value : J.isWildcard(e) ? "*" : e.variable.value), r = new Set(n);
		for (let t of e) if (r.has(t)) throw Error(`Target id of 'AS' (?${t}) already used in subquery`);
	}
}
function kb(e, t) {
	function n(e) {
		kb(e, t);
	}
	if (e !== void 0) {
		if (Array.isArray(e)) for (let t of e) n(t);
		else if (J.isQuery(e)) J.isQuerySelect(e) || J.isQueryDescribe(e) ? n([
			...e.variables.some((e) => J.isWildcard(e)) ? [e.where] : e.variables,
			e.solutionModifiers.group,
			e.values
		]) : n(e.solutionModifiers.group);
		else if (J.isTriple(e)) n([
			e.subject,
			e.predicate,
			e.object
		]);
		else if (J.isPathPure(e)) n(e.items);
		else if (J.isTripleCollection(e)) n([e.identifier, ...e.triples]);
		else if (J.isSolutionModifierGroup(e)) n(e.groupings.filter((e) => "variable" in e).map((e) => e.variable));
		else if (J.isSolutionModifierHaving(e)) n(e.having);
		else if (J.isSolutionModifierOrder(e)) n(e.orderDefs.map((e) => e.expression));
		else if (J.isPatternValues(e)) for (let n of Object.keys(e.values.at(0) ?? {})) t.add(n);
		else J.isPatternBgp(e) ? n(e.triples) : J.isPatternGroup(e) || J.isPatternUnion(e) || J.isPatternOptional(e) ? n(e.patterns) : J.isPatternService(e) || J.isPatternGraph(e) ? n([e.name, ...e.patterns]) : J.isPatternBind(e) ? n(e.variable) : J.isTermVariable(e) && t.add(e.value);
	}
}
function Ab(e) {
	for (let [t, n] of e.entries()) if (J.isPatternBind(n) && t > 0 && J.isPatternBgp(e[t - 1])) {
		let r = e[t - 1], i = [];
		if (wb.visitNodeSpecific(r, {}, { term: { variable: { visitor: (e) => {
			i.push(e);
		} } } }), i.some((e) => e.value === n.variable.value)) throw Error(`Variable used to bind is already bound (?${n.variable.value})`);
	}
	let t = /* @__PURE__ */ new Set();
	for (let n of e) if (J.isPatternBind(n)) {
		if (t.has(n.variable.value)) throw Error(`Variable used to bind is already bound (?${n.variable.value})`);
	} else kb(n, t);
}
function jb(e) {
	let t = /* @__PURE__ */ new Set();
	for (let n of e.updates) {
		if (!n.operation) continue;
		let e = n.operation;
		if (e.subType === "insertdata") {
			let n = /* @__PURE__ */ new Set();
			wb.visitNodeSpecific(e, {}, { term: { blankNode: { visitor: (e) => {
				if (n.add(e.label), t.has(e.label)) throw Error("Detected reuse blank node across different INSERT DATA clauses");
			} } } });
			for (let e of n) t.add(e);
		}
	}
}
function Mb(e) {
	let t = /* @__PURE__ */ new Map(), n = [{}], r = () => n.at(-1);
	function i(e) {
		let t = /* @__PURE__ */ new Set();
		return wb.visitNodeSpecific(e, {}, { term: { blankNode: { visitor: (e) => {
			t.add(e.label);
		} } } }), t;
	}
	wb.visitNodeSpecific(e, {
		query: { preVisitor: () => ({ continue: !1 }) },
		pattern: {
			preVisitor: () => (n.push({}), {}),
			visitor: () => {
				n.pop(), n[n.length - 1] = {};
			}
		}
	}, { pattern: {
		bgp: {
			preVisitor: (e) => {
				let n = r();
				for (let r of i(e)) {
					let e = t.get(r);
					if (e !== void 0 && e !== n) throw Error(`Detected reuse of blank node across two different basic graph patterns (_:${r.replace(/^[eg]_/u, "")})`);
					t.set(r, n);
				}
				return { continue: !1 };
			},
			visitor: () => {}
		},
		filter: {
			preVisitor: () => (n.push({}), {}),
			visitor: () => {
				n.pop();
			}
		}
	} });
}
//#endregion
//#region ../../wt-query-graph/web/node_modules/@traqula/rules-sparql-1-1/dist/esm/lib/grammar/literals.js
var Nb = {
	"	": "\\t",
	"\n": "\\n",
	"\r": "\\r",
	"\b": "\\b",
	"\f": "\\f",
	"\"": "\\\"",
	"\\": "\\\\"
};
function Pb(e) {
	return `"${e.replaceAll(/["\\\t\n\r\b\f]/gu, (e) => Nb[e])}"`;
}
var Fb = {
	name: "rdfLiteral",
	impl: ({ ACTION: e, SUBRULE1: t, CONSUME: n, OPTION: r, OR: i }) => (a) => {
		let o = t(Vb);
		return r(() => i([{ ALT: () => {
			let t = n(zv);
			return e(() => a.astFactory.termLiteral(a.astFactory.sourceLocation(o, t), o.value, t.image.slice(1).toLowerCase()));
		} }, { ALT: () => {
			n(jv);
			let r = t(Y);
			return e(() => a.astFactory.termLiteral(a.astFactory.sourceLocation(o, r), o.value, r));
		} }])) ?? o;
	},
	gImpl: ({ SUBRULE: e, PRINT: t, PRINT_WORD: n }) => (r, { astFactory: i }) => {
		!r.langOrIri || typeof r.langOrIri == "string" ? (i.printFilter(r, () => {
			n(""), t(Pb(r.value));
		}), typeof r.langOrIri == "string" && i.printFilter(r, () => t("@", r.langOrIri))) : i.isSourceLocationNoMaterialize(r.langOrIri.loc) ? i.printFilter(r, () => {
			n(r.value);
		}) : (i.printFilter(r, () => {
			n(""), t(Pb(r.value), "^^");
		}), e(Y, r.langOrIri));
	}
}, Ib = {
	name: "numericLiteral",
	impl: ({ SUBRULE: e, OR: t }) => () => t([
		{ ALT: () => e(Lb) },
		{ ALT: () => e(Rb) },
		{ ALT: () => e(zb) }
	])
}, Lb = {
	name: "numericLiteralUnsigned",
	impl: ({ ACTION: e, CONSUME: t, OR: n }) => (r) => {
		let i = n([
			{ ALT: () => [t(Bv), q.INTEGER] },
			{ ALT: () => [t(Vv), q.DECIMAL] },
			{ ALT: () => [t(Hv), q.DOUBLE] }
		]);
		return e(() => r.astFactory.termLiteral(r.astFactory.sourceLocation(i[0]), i[0].image, r.astFactory.termNamed(r.astFactory.sourceLocation(), i[1])));
	}
}, Rb = {
	name: "numericLiteralPositive",
	impl: ({ ACTION: e, CONSUME: t, OR: n }) => (r) => {
		let i = n([
			{ ALT: () => [t(Uv), q.INTEGER] },
			{ ALT: () => [t(Wv), q.DECIMAL] },
			{ ALT: () => [t(Gv), q.DOUBLE] }
		]);
		return e(() => r.astFactory.termLiteral(r.astFactory.sourceLocation(i[0]), i[0].image, r.astFactory.termNamed(r.astFactory.sourceLocation(), i[1])));
	}
}, zb = {
	name: "numericLiteralNegative",
	impl: ({ ACTION: e, CONSUME: t, OR: n }) => (r) => {
		let i = n([
			{ ALT: () => [t(Kv), q.INTEGER] },
			{ ALT: () => [t(qv), q.DECIMAL] },
			{ ALT: () => [t(Jv), q.DOUBLE] }
		]);
		return e(() => r.astFactory.termLiteral(r.astFactory.sourceLocation(i[0]), i[0].image, r.astFactory.termNamed(r.astFactory.sourceLocation(), i[1])));
	}
}, Bb = {
	name: "booleanLiteral",
	impl: ({ ACTION: e, CONSUME: t, OR: n }) => (r) => {
		let i = n([{ ALT: () => t(Yy) }, { ALT: () => t(Xy) }]);
		return e(() => r.astFactory.termLiteral(r.astFactory.sourceLocation(i), i.image.toLowerCase(), r.astFactory.termNamed(r.astFactory.sourceLocation(), q.BOOLEAN)));
	}
}, Vb = {
	name: "string",
	impl: ({ ACTION: e, CONSUME: t, OR: n }) => (r) => {
		let i = n([
			{ ALT: () => {
				let e = t(Yv);
				return [e, e.image.slice(1, -1)];
			} },
			{ ALT: () => {
				let e = t(Xv);
				return [e, e.image.slice(1, -1)];
			} },
			{ ALT: () => {
				let e = t(Zv);
				return [e, e.image.slice(3, -3)];
			} },
			{ ALT: () => {
				let e = t(Qv);
				return [e, e.image.slice(3, -3)];
			} }
		]);
		return e(() => {
			let e = r.astFactory, t = i[1].replaceAll(/\\([tnrbf"'\\])/gu, (e, t) => {
				switch (t) {
					case "t": return "	";
					case "n": return "\n";
					case "r": return "\r";
					case "b": return "\b";
					case "f": return "\f";
					default: return t;
				}
			});
			return e.termLiteral(e.sourceLocation(i[0]), t);
		});
	}
}, Y = {
	name: "iri",
	impl: ({ SUBRULE: e, OR: t }) => () => t([{ ALT: () => e(Hb) }, { ALT: () => e(Ub) }]),
	gImpl: ({ SUBRULE: e }) => (t, { astFactory: n }) => n.isTermNamedPrefixed(t) ? e(Ub, t) : e(Hb, t)
}, Hb = {
	name: "iriFull",
	impl: ({ ACTION: e, CONSUME: t }) => (n) => {
		let r = t(Nv);
		return e(() => n.astFactory.termNamed(n.astFactory.sourceLocation(r), r.image.slice(1, -1)));
	},
	gImpl: ({ PRINT: e }) => (t, { astFactory: n }) => {
		n.printFilter(t, () => e("<", t.value, ">"));
	}
}, Ub = {
	name: "prefixedName",
	impl: ({ ACTION: e, CONSUME: t, OR: n }) => (r) => {
		function i(e) {
			if (!r.skipValidation && r.prefixes[e] === void 0) throw Error(`Unknown prefix: ${e}`);
		}
		return n([{ ALT: () => {
			let n = t(Pv);
			return e(() => {
				let e = n.image.indexOf(":"), t = n.image.slice(0, e), a = n.image.slice(e + 1);
				return i(t), r.astFactory.termNamed(r.astFactory.sourceLocation(n), a, t);
			});
		} }, { ALT: () => {
			let n = t(Fv);
			return e(() => {
				let e = n.image.slice(0, -1);
				return i(e), r.astFactory.termNamed(r.astFactory.sourceLocation(n), "", e);
			});
		} }]);
	},
	gImpl: ({ PRINT: e }) => (t, { astFactory: n }) => {
		n.printFilter(t, () => e(t.prefix, ":", t.value));
	}
}, Wb = {
	name: "blankNode",
	impl: ({ ACTION: e, CONSUME: t, OR: n }) => (r) => {
		let i = n([{ ALT: () => {
			let n = t(Iv);
			return e(() => r.astFactory.termBlank(n.image.slice(2), r.astFactory.sourceLocation(n)));
		} }, { ALT: () => {
			let n = t(ny);
			return e(() => r.astFactory.termBlank(void 0, r.astFactory.sourceLocation(n)));
		} }]);
		return e(() => {
			if (!r.parseMode.has("canCreateBlankNodes")) throw Error("Blank nodes are not allowed in this context");
		}), i;
	},
	gImpl: ({ PRINT: e }) => (t, { astFactory: n }) => {
		n.printFilter(t, () => e("_:", t.label.replace(/^e_/u, "")));
	}
}, Gb = {
	name: "VerbA",
	impl: ({ ACTION: e, CONSUME: t }) => (n) => {
		let r = t(Jy);
		return e(() => n.astFactory.termNamed(n.astFactory.sourceLocation(r), q.TYPE, void 0));
	}
}, Kb = {
	name: "prologue",
	impl: ({ SUBRULE: e, MANY: t, OR: n }) => () => {
		let r = [];
		return t(() => n([{ ALT: () => r.push(e(qb)) }, { ALT: () => r.push(e(Jb)) }])), r;
	},
	gImpl: ({ SUBRULE: e }) => (t, { astFactory: n }) => {
		for (let r of t) n.isContextDefinitionBase(r) ? e(qb, r) : e(Jb, r);
	}
}, qb = {
	name: "baseDecl",
	impl: ({ ACTION: e, CONSUME: t, SUBRULE: n }) => (r) => {
		let i = t(iy), a = n(Hb);
		return e(() => r.astFactory.contextDefinitionBase(r.astFactory.sourceLocation(i, a), a));
	},
	gImpl: ({ SUBRULE: e, PRINT_ON_EMPTY: t, NEW_LINE: n }) => (r, { astFactory: i }) => {
		i.printFilter(r, () => t("BASE ")), e(Y, r.value), i.printFilter(r, () => n());
	}
}, Jb = {
	name: "prefixDecl",
	impl: ({ ACTION: e, CONSUME: t, SUBRULE: n }) => (r) => {
		let i = t(ay), a = t(Fv).image.slice(0, -1), o = n(Hb);
		return e(() => (r.prefixes[a] = o.value, r.astFactory.contextDefinitionPrefix(r.astFactory.sourceLocation(i, o), a, o)));
	},
	gImpl: ({ SUBRULE: e, PRINT_ON_EMPTY: t, NEW_LINE: n }) => (r, { astFactory: i }) => {
		i.printFilter(r, () => {
			t("PREFIX ", `${r.key}: `);
		}), e(Y, r.value), i.printFilter(r, () => n());
	}
}, Yb = {
	name: "verb",
	impl: ({ SUBRULE: e, OR: t }) => () => t([{ ALT: () => e(Zb) }, { ALT: () => e(Gb) }])
}, Xb = {
	name: "varOrTerm",
	impl: ({ SUBRULE: e, OR: t }) => (n) => t([{
		GATE: () => n.parseMode.has("canParseVars"),
		ALT: () => e(X)
	}, { ALT: () => e(Qb) }]),
	gImpl: ({ SUBRULE: e }) => (t, { astFactory: n }) => n.isTermVariable(t) ? e(X, t) : e(Qb, t)
}, Zb = {
	name: "varOrIri",
	impl: ({ SUBRULE: e, OR: t }) => (n) => t([{
		GATE: () => n.parseMode.has("canParseVars"),
		ALT: () => e(X)
	}, { ALT: () => e(Y) }])
}, X = {
	name: "var",
	impl: ({ ACTION: e, CONSUME: t, OR: n }) => (r) => {
		let i = n([{ ALT: () => t(Lv) }, { ALT: () => t(Rv) }]);
		return e(() => r.astFactory.termVariable(i.image.slice(1), r.astFactory.sourceLocation(i)));
	},
	gImpl: ({ PRINT: e }) => (t, { astFactory: n }) => {
		n.printFilter(t, () => e(`?${t.value}`));
	}
}, Qb = {
	name: "graphTerm",
	impl: ({ ACTION: e, SUBRULE: t, CONSUME: n, OR: r }) => (i) => r([
		{ ALT: () => t(Y) },
		{ ALT: () => t(Fb) },
		{ ALT: () => t(Ib) },
		{ ALT: () => t(Bb) },
		{
			GATE: () => i.parseMode.has("canCreateBlankNodes"),
			ALT: () => t(Wb)
		},
		{ ALT: () => {
			let t = n(ty);
			return e(() => i.astFactory.termNamed(i.astFactory.sourceLocation(t), q.NIL));
		} }
	]),
	gImpl: ({ SUBRULE: e }) => (t, { astFactory: n }) => {
		n.isTermNamed(t) ? e(Y, t) : n.isTermLiteral(t) ? e(Fb, t) : e(Wb, t);
	}
};
//#endregion
//#region ../../wt-query-graph/web/node_modules/@traqula/rules-sparql-1-1/dist/esm/lib/grammar/dataSetClause.js
function $b(e, t) {
	return {
		name: e,
		impl: ({ ACTION: e, SUBRULE: n, CONSUME: r, OR: i }) => (a) => {
			let o = r(t);
			return i([{ ALT: () => {
				let t = n(tx);
				return e(() => a.astFactory.wrap({
					clauseType: "default",
					value: t
				}, a.astFactory.sourceLocation(o, t)));
			} }, { ALT: () => {
				let t = n(ox);
				return e(() => a.astFactory.wrap({
					clauseType: "named",
					value: t.val
				}, a.astFactory.sourceLocation(o, t)));
			} }]);
		}
	};
}
var ex = $b("datasetClause", py), tx = {
	name: "defaultGraphClause",
	impl: ({ SUBRULE: e }) => () => e(sx)
}, nx = $b("usingClause", By);
function rx(e, t, n) {
	return {
		name: e,
		impl: ({ ACTION: e, MANY: n, SUBRULE: r }) => (i) => {
			let a = [];
			return n(() => {
				let e = r(t);
				a.push(e);
			}), e(() => i.astFactory.datasetClauses(a.map((e) => e.val), i.astFactory.sourceLocation(...a)));
		},
		gImpl: ({ SUBRULE: e, PRINT_WORD: t }) => (r, { astFactory: i }) => {
			for (let a of r.clauses) i.printFilter(r, () => t(n)), a.clauseType === "named" && i.printFilter(r, () => t("NAMED")), e(Y, a.value);
		}
	};
}
var ix = rx("datasetClauses", ex, "FROM"), ax = rx("usingClauses", nx, "USING"), ox = {
	name: "namedGraphClause",
	impl: ({ ACTION: e, SUBRULE: t, CONSUME: n }) => (r) => {
		let i = n(C_), a = t(sx);
		return e(() => r.astFactory.wrap(a, r.astFactory.sourceLocation(i, a)));
	}
}, sx = {
	name: "sourceSelector",
	impl: ({ SUBRULE: e }) => () => e(Y)
};
//#endregion
//#region ../../wt-query-graph/web/node_modules/@traqula/rules-sparql-1-1/dist/esm/lib/expressionHelpers.js
function Z(e) {
	return {
		name: z(e.name),
		impl: ({ ACTION: t, SUBRULE: n, CONSUME: r }) => (i) => {
			let a = r(e);
			r(W);
			let o = n($), s = r(G);
			return t(() => i.astFactory.expressionOperation(a.image, [o], i.astFactory.sourceLocation(a, s)));
		}
	};
}
function cx(e) {
	return {
		name: z(e.name),
		impl: ({ ACTION: t, CONSUME: n, SUBRULE1: r, SUBRULE2: i }) => (a) => {
			let o = n(e);
			n(W);
			let s = r($);
			n(dv);
			let c = i($), l = n(G);
			return t(() => a.astFactory.expressionOperation(o.image, [s, c], a.astFactory.sourceLocation(o, l)));
		}
	};
}
function lx(e) {
	return {
		name: z(e.name),
		impl: ({ ACTION: t, CONSUME: n, CONSUME1: r, CONSUME2: i, SUBRULE1: a, SUBRULE2: o, SUBRULE3: s }) => (c) => {
			let l = n(e);
			n(W);
			let u = a($);
			r(dv);
			let d = o($);
			i(dv);
			let f = s($), p = n(G);
			return t(() => c.astFactory.expressionOperation(l.image, [
				u,
				d,
				f
			], c.astFactory.sourceLocation(l, p)));
		}
	};
}
function ux(e) {
	return {
		name: z(e.name),
		impl: ({ ACTION: t, SUBRULE: n, CONSUME: r }) => (i) => {
			let a = r(e);
			r(W);
			let o = n(X), s = r(G);
			return t(() => i.astFactory.expressionOperation(a.image, [o], i.astFactory.sourceLocation(a, s)));
		}
	};
}
function dx(e) {
	return {
		name: z(e.name),
		impl: ({ ACTION: t, CONSUME: n, OR: r, SUBRULE: i }) => (a) => {
			let o = n(e);
			return r([{ ALT: () => {
				n(W);
				let e = i($), r = n(G);
				return t(() => a.astFactory.expressionOperation(o.image, [e], a.astFactory.sourceLocation(o, r)));
			} }, { ALT: () => {
				let e = n(ty);
				return t(() => a.astFactory.expressionOperation(o.image, [], a.astFactory.sourceLocation(o, e)));
			} }]);
		}
	};
}
function fx(e) {
	return {
		name: z(e.name),
		impl: ({ ACTION: t, CONSUME: n }) => (r) => {
			let i = n(e), a = n(ty);
			return t(() => r.astFactory.expressionOperation(i.image, [], r.astFactory.sourceLocation(i, a)));
		}
	};
}
function px(e) {
	return {
		name: z(e.name),
		impl: ({ ACTION: t, CONSUME: n, SUBRULE: r }) => (i) => {
			let a = n(e), o = r(IC);
			return t(() => i.astFactory.expressionOperation(a.image, o.val, i.astFactory.sourceLocation(a, o)));
		}
	};
}
function mx(e) {
	return {
		name: z(e.name),
		impl: ({ ACTION: t, CONSUME: n, SUBRULE1: r, SUBRULE2: i, SUBRULE3: a, CONSUME1: o, OPTION: s, CONSUME2: c }) => (l) => {
			let u = n(e);
			n(W);
			let d = r($);
			o(dv);
			let f = i($), p = s(() => (c(dv), a($))), m = n(G);
			return t(() => l.astFactory.expressionOperation(u.image, p ? [
				d,
				f,
				p
			] : [d, f], l.astFactory.sourceLocation(u, m)));
		}
	};
}
function hx(e) {
	return {
		name: z(e.name),
		impl: ({ ACTION: t, CONSUME: n, SUBRULE1: r, SUBRULE2: i, SUBRULE3: a, SUBRULE4: o, CONSUME1: s, OPTION: c, CONSUME2: l, CONSUME3: u }) => (d) => {
			let f = n(e);
			n(W);
			let p = r($);
			s(dv);
			let m = i($);
			l(dv);
			let h = a($), g = c(() => (u(dv), o($))), _ = n(G);
			return t(() => d.astFactory.expressionOperation(f.image, g ? [
				p,
				m,
				h,
				g
			] : [
				p,
				m,
				h
			], d.astFactory.sourceLocation(f, _)));
		}
	};
}
function gx(e) {
	return {
		name: z(e.name),
		impl: ({ ACTION: t, SUBRULE: n, CONSUME: r }) => (i) => {
			let a = r(e), o = n(Q);
			return t(() => i.astFactory.expressionPatternOperation(a.image, o, i.astFactory.sourceLocation(a, o)));
		}
	};
}
function _x(e) {
	return {
		name: z(e.name),
		impl: ({ ACTION: t, CONSUME: n, SUBRULE: r, OPTION: i }) => (a) => {
			let o = n(e);
			n(W);
			let s = i(() => n(sy)), c = r($), l = n(G);
			return t(() => a.astFactory.aggregate(o.image, s !== void 0, c, void 0, a.astFactory.sourceLocation(o, l)));
		}
	};
}
//#endregion
//#region ../../wt-query-graph/web/node_modules/@traqula/rules-sparql-1-1/dist/esm/lib/grammar/builtIn.js
var vx = Z(hg), yx = Z(gg), bx = cx(_g), xx = Z(vg), Sx = ux(yg), Cx = Z(bg), wx = Z(xg), Tx = dx(Sg), Ex = fx(Cg), Dx = Z(wg), Ox = Z(Tg), kx = Z(Eg), Ax = Z(Dg), jx = px(Og), Mx = Z(kg), Nx = Z(Ag), Px = Z(jg), Fx = Z(Mg), Ix = cx(Ng), Lx = cx(Pg), Rx = cx(Fg), zx = cx(Ig), Bx = cx(Lg), Vx = Z(Rg), Hx = Z(zg), Ux = Z(Bg), Wx = Z(Vg), Gx = Z(Hg), Kx = Z(Ug), qx = Z(Wg), Jx = Z(Gg), Yx = fx(Kg), Xx = fx(qg), Zx = fx(Jg), Qx = Z(Yg), $x = Z(Xg), eS = Z(Zg), tS = Z(Qg), nS = Z($g), rS = px(e_), iS = lx(t_), aS = cx(n_), oS = cx(r_), sS = cx(i_), cS = Z(a_), lS = Z(o_), uS = Z(s_), dS = Z(c_), fS = Z(l_);
function pS(e) {
	return [
		{ ALT: () => e(DS) },
		{ ALT: () => e(vx) },
		{ ALT: () => e(yx) },
		{ ALT: () => e(bx) },
		{ ALT: () => e(xx) },
		{ ALT: () => e(Sx) },
		{ ALT: () => e(Cx) },
		{ ALT: () => e(wx) },
		{ ALT: () => e(Tx) },
		{ ALT: () => e(Ex) },
		{ ALT: () => e(Dx) },
		{ ALT: () => e(Ox) },
		{ ALT: () => e(kx) },
		{ ALT: () => e(Ax) },
		{ ALT: () => e(jx) },
		{ ALT: () => e(gS) },
		{ ALT: () => e(Mx) },
		{ ALT: () => e(_S) },
		{ ALT: () => e(Nx) },
		{ ALT: () => e(Px) },
		{ ALT: () => e(Fx) },
		{ ALT: () => e(Ix) },
		{ ALT: () => e(Lx) },
		{ ALT: () => e(Rx) },
		{ ALT: () => e(zx) },
		{ ALT: () => e(Bx) },
		{ ALT: () => e(Vx) },
		{ ALT: () => e(Hx) },
		{ ALT: () => e(Ux) },
		{ ALT: () => e(Wx) },
		{ ALT: () => e(Gx) },
		{ ALT: () => e(Kx) },
		{ ALT: () => e(qx) },
		{ ALT: () => e(Jx) },
		{ ALT: () => e(Yx) },
		{ ALT: () => e(Xx) },
		{ ALT: () => e(Zx) },
		{ ALT: () => e(Qx) },
		{ ALT: () => e($x) },
		{ ALT: () => e(eS) },
		{ ALT: () => e(tS) },
		{ ALT: () => e(nS) },
		{ ALT: () => e(rS) },
		{ ALT: () => e(iS) },
		{ ALT: () => e(aS) },
		{ ALT: () => e(oS) },
		{ ALT: () => e(sS) },
		{ ALT: () => e(cS) },
		{ ALT: () => e(lS) },
		{ ALT: () => e(uS) },
		{ ALT: () => e(dS) },
		{ ALT: () => e(fS) },
		{ ALT: () => e(hS) },
		{ ALT: () => e(vS) },
		{ ALT: () => e(yS) }
	];
}
var mS = {
	name: "builtInCall",
	impl: ({ OR: e, SUBRULE: t, cache: n }) => () => {
		let r = n.get(mS);
		if (r) return e(r);
		let i = pS(t);
		return n.set(mS, i), e(i);
	}
}, hS = mx(u_), gS = mx(d_), _S = hx(f_), vS = gx(p_), yS = gx(m_), bS = {
	name: z(h_.name),
	impl: ({ ACTION: e, CONSUME: t, SUBRULE: n, OR: r, OPTION: i }) => (a) => {
		let o = t(h_);
		t(W);
		let s = i(() => t(sy)), c = r([{ ALT: () => {
			let n = t(yv);
			return e(() => a.astFactory.wildcard(a.astFactory.sourceLocation(n)));
		} }, { ALT: () => n($) }]), l = t(G);
		return e(() => {
			let e = a.astFactory;
			return a.astFactory.isWildcard(c), e.aggregate(o.image, !!s, c, void 0, a.astFactory.sourceLocation(o, l));
		});
	}
}, xS = _x(g_), SS = _x(__), CS = _x(v_), wS = _x(y_), TS = _x(b_), ES = {
	name: z(x_.name),
	impl: ({ ACTION: e, CONSUME: t, OPTION1: n, SUBRULE: r, OPTION2: i }) => (a) => {
		let o = t(x_);
		t(W);
		let s = n(() => t(sy)), c = r($), l = i(() => (t(fv), t($y), t(Tv), r(Vb))), u = t(G);
		return e(() => {
			let e = a.astFactory;
			return e.aggregate(o.image, !!s, c, l?.value ?? " ", e.sourceLocation(o, u));
		});
	}
}, DS = {
	name: "aggregate",
	impl: ({ ACTION: e, SUBRULE: t, OR: n }) => (r) => {
		let i = e(() => r.parseMode.has("inAggregate"));
		e(() => r.parseMode.add("inAggregate"));
		let a = n([
			{ ALT: () => t(bS) },
			{ ALT: () => t(xS) },
			{ ALT: () => t(SS) },
			{ ALT: () => t(CS) },
			{ ALT: () => t(wS) },
			{ ALT: () => t(TS) },
			{ ALT: () => t(ES) }
		]);
		return e(() => !i && r.parseMode.delete("inAggregate")), e(() => {
			if (!r.parseMode.has("canParseAggregate")) throw Error("Aggregates are only allowed in SELECT, HAVING, and ORDER BY clauses.");
			if (r.parseMode.has("inAggregate")) throw Error("An aggregate function is not allowed within an aggregate function.");
		}), a;
	},
	gImpl: ({ SUBRULE: e, PRINT_WORD: t }) => (n, { astFactory: r }) => {
		r.printFilter(n, () => {
			t(n.aggregation.toUpperCase(), "("), n.distinct && t("DISTINCT");
		});
		let i = n.expression[0];
		r.isWildcard(i) ? r.printFilter(n, () => t("*")) : e($, i), r.isExpressionAggregateSeparator(n) && r.printFilter(n, () => t(";", "SEPARATOR", "=", Pb(n.separator))), r.printFilter(n, () => t(")"));
	}
}, OS = {
	name: "path",
	impl: ({ SUBRULE: e }) => () => e(NS)
}, kS = {
	name: "path",
	gImpl: ({ PRINT: e, SUBRULE: t }) => (n, { astFactory: r }, i = !0) => {
		if (r.isTerm(n) && r.isTermNamed(n)) t(Y, n);
		else {
			switch (r.printFilter(n, () => i && e("(")), n.subType) {
				case "|":
				case "/": {
					let [a, ...o] = n.items;
					t(kS, a, i);
					for (let a of o) r.printFilter(n, () => e(n.subType)), t(kS, a, i);
					break;
				}
				case "^":
					r.printFilter(n, () => e("^")), t(kS, n.items[0], i);
					break;
				case "?":
				case "*":
				case "+":
					t(kS, n.items[0], i), r.printFilter(n, () => e(n.subType));
					break;
				case "!": r.printFilter(n, () => e("!")), r.printFilter(n, () => e("(")), t(kS, n.items[0], !1), r.printFilter(n, () => e(")"));
			}
			r.printFilter(n, () => i && e(")"));
		}
	}
};
function AS(e, t, n, r) {
	return {
		name: e,
		impl: ({ ACTION: e, CONSUME: i, SUBRULE1: a, SUBRULE2: o, MANY: s }) => (c) => {
			let l = a(r), u = l, d = [];
			return s(() => {
				i(t), u = o(r), d.push(u);
			}), e(() => d.length === 0 ? l : c.astFactory.path(n, [l, ...d], c.astFactory.sourceLocation(l, u)));
		}
	};
}
var jS = {
	name: "pathEltOrInverse",
	impl: ({ ACTION: e, CONSUME: t, SUBRULE1: n, SUBRULE2: r, OR: i }) => (a) => i([{ ALT: () => n(PS) }, { ALT: () => {
		let n = t(_v), i = r(PS);
		return e(() => a.astFactory.path("^", [i], a.astFactory.sourceLocation(n, i)));
	} }])
}, MS = AS("pathSequence", gv, "/", jS), NS = AS("pathAlternative", hv, "|", MS), PS = {
	name: "pathElt",
	impl: ({ ACTION: e, SUBRULE: t, OPTION: n }) => (r) => {
		let i = t(IS), a = n(() => t(FS));
		return e(() => a === void 0 ? i : r.astFactory.path(a.image, [i], r.astFactory.sourceLocation(i, a)));
	}
}, FS = {
	name: "pathMod",
	impl: ({ CONSUME: e, OR: t }) => () => t([
		{ ALT: () => e(vv) },
		{ ALT: () => e(yv) },
		{ ALT: () => e(bv) }
	])
}, IS = {
	name: "pathPrimary",
	impl: ({ SUBRULE: e, CONSUME: t, OR: n }) => () => n([
		{ ALT: () => e(Y) },
		{ ALT: () => e(Gb) },
		{ ALT: () => e(LS) },
		{ ALT: () => {
			t(W);
			let n = e(OS);
			return t(G), n;
		} }
	])
}, LS = {
	name: "pathNegatedPropertySet",
	impl: ({ ACTION: e, CONSUME: t, SUBRULE1: n, SUBRULE2: r, SUBRULE3: i, OR: a, MANY: o }) => (s) => {
		let c = t(Sv);
		return a([{ ALT: () => {
			let t = n(RS);
			return e(() => s.astFactory.path("!", [t], s.astFactory.sourceLocation(c, t)));
		} }, { ALT: () => {
			let n = t(W), a = r(RS), l = [];
			o(() => {
				t(hv);
				let e = i(RS);
				l.push(e);
			});
			let u = t(G);
			return e(() => {
				let e = s.astFactory;
				return l.length === 0 ? e.path("!", [a], e.sourceLocation(c, u)) : e.path("!", [e.path("|", [a, ...l], e.sourceLocation(n, u))], e.sourceLocation(c, u));
			});
		} }]);
	}
}, RS = {
	name: "pathOneInPropertySet",
	impl: ({ ACTION: e, CONSUME: t, SUBRULE1: n, SUBRULE2: r, OR1: i, OR2: a }) => (o) => i([
		{ ALT: () => n(Y) },
		{ ALT: () => n(Gb) },
		{ ALT: () => {
			let n = t(_v), i = a([{ ALT: () => r(Y) }, { ALT: () => r(Gb) }]);
			return e(() => o.astFactory.path("^", [i], o.astFactory.sourceLocation(n, i)));
		} }
	])
};
//#endregion
//#region ../../wt-query-graph/web/node_modules/@traqula/rules-sparql-1-1/dist/esm/lib/grammar/tripleBlock.js
function zS(e) {
	return ({ ACTION: t, AT_LEAST_ONE: n, SUBRULE: r, CONSUME: i, OPTION: a }) => (o) => {
		let s = [], c = !0, l;
		return n({
			GATE: () => c,
			DEF: () => {
				c = !1;
				let n = r(e);
				t(() => {
					s.push(...n);
				}), a(() => {
					l = i(uv), c = !0;
				});
			}
		}), t(() => o.astFactory.patternBgp(s, o.astFactory.sourceLocation(...s, l)));
	};
}
var BS = {
	name: "triplesBlock",
	impl: (e) => (t) => zS(US)(e)(t),
	gImpl: ({ SUBRULE: e, PRINT_WORD: t, HANDLE_LOC: n, NEW_LINE: r }) => (i, { astFactory: a }) => {
		for (let [o, s] of i.triples.entries()) n(s, () => {
			let n = i.triples.at(o + 1);
			a.isTripleCollection(s) ? (e(gC, s), a.printFilter(s, () => {
				t("."), r();
			})) : (e(gC, s.subject), a.printFilter(s, () => t("")), a.isTerm(s.predicate) && a.isTermVariable(s.predicate) ? e(Xb, s.predicate) : e(kS, s.predicate, void 0), a.printFilter(s, () => t("")), e(gC, s.object), n === void 0 || a.isTripleCollection(n) || !a.isSourceLocationNoMaterialize(n.subject.loc) ? a.printFilter(i, () => {
				t("."), r();
			}) : a.isSourceLocationNoMaterialize(n.predicate.loc) ? a.printFilter(i, () => t(",")) : a.printFilter(i, () => {
				t(";"), r();
			}));
		});
	}
};
function VS(e, t) {
	return {
		name: e,
		impl: ({ ACTION: e, SUBRULE: n, OR: r }) => (i) => r([{ ALT: () => {
			let r = n(Xb), a = n(t ? XS : YS, e(() => i.astFactory.dematerialized(r)));
			return e(() => (a[0].subject = r, a[0].loc = i.astFactory.sourceLocation(r, a[0]), a));
		} }, { ALT: () => {
			let r = n(t ? uC : lC), a = n(t ? qS : KS, e(() => i.astFactory.graphNodeIdentifier(r)));
			return e(() => a.length === 0 ? [r] : (a[0].subject = r, a[0].loc = i.astFactory.sourceLocation(r, a[0]), a));
		} }])
	};
}
var HS = VS("triplesSameSubject", !1), US = VS("triplesSameSubjectPath", !0), WS = {
	name: "triplesTemplate",
	impl: zS(HS)
};
function GS(e, t) {
	return {
		name: e,
		impl: ({ SUBRULE: e, OPTION: n }) => (r, i) => n(() => e(t ? XS : YS, i)) ?? []
	};
}
var KS = GS("propertyList", !1), qS = GS("propertyListPath", !0);
function JS(e, t) {
	return {
		name: e,
		impl: ({ ACTION: e, CONSUME: n, AT_LEAST_ONE: r, SUBRULE1: i, MANY2: a, OR1: o }) => (s, c) => {
			let l = [], u = !0;
			return r({
				GATE: () => u,
				DEF: () => {
					u = !1;
					let r = t ? o([{ ALT: () => i(ZS) }, { ALT: () => i(QS) }]) : i(Yb), s = i(t ? tC : eC, c, r);
					a(() => {
						n(fv), u = !0;
					}), e(() => {
						l.push(...s);
					});
				}
			}), l;
		}
	};
}
var YS = JS("propertyListNotEmpty", !1), XS = JS("propertyListPathNotEmpty", !0), ZS = {
	name: "verbPath",
	impl: ({ SUBRULE: e }) => () => e(OS)
}, QS = {
	name: "verbSimple",
	impl: ({ SUBRULE: e }) => () => e(X)
};
function $S(e, t) {
	return {
		name: e,
		impl: ({ ACTION: e, SUBRULE: n, AT_LEAST_ONE_SEP: r }) => (i, a, o) => {
			let s = [];
			return r({
				SEP: dv,
				DEF: () => {
					let r = n(t ? iC : rC, a, o);
					e(() => {
						s.push(r);
					});
				}
			}), s;
		}
	};
}
var eC = $S("objectList", !1), tC = $S("objectListPath", !0);
function nC(e, t) {
	return {
		name: e,
		impl: ({ ACTION: e, SUBRULE: n }) => (r, i, a) => {
			let o = n(t ? gC : hC);
			return e(() => r.astFactory.triple(i, a, o));
		}
	};
}
var rC = nC("object", !1), iC = nC("objectPath", !0);
function aC(e, t) {
	return {
		name: e,
		impl: ({ ACTION: e, AT_LEAST_ONE: n, SUBRULE: r, CONSUME: i }) => (a) => {
			let o = [], s = i(W);
			n(() => {
				o.push(r(t ? gC : hC));
			});
			let c = i(G);
			return e(() => {
				let e = a.astFactory, t = [], n = e.termNamed(e.sourceLocation(), q.FIRST, void 0), r = e.termNamed(e.sourceLocation(), q.REST, void 0), i = e.termNamed(e.sourceLocation(), q.NIL, void 0), l = e.termBlank(void 0, e.sourceLocation()), u = l;
				for (let [a, s] of o.entries()) {
					let c = a === o.length - 1, l = e.triple(u, n, s);
					if (t.push(l), c) {
						let n = e.triple(u, r, i);
						t.push(n);
					} else {
						let n = e.termBlank(void 0, e.sourceLocation()), i = e.triple(u, r, n);
						t.push(i), u = n;
					}
				}
				return e.tripleCollectionList(l, t, e.sourceLocation(s, c));
			});
		},
		gImpl: ({ SUBRULE: e, PRINT_WORD: t }) => (n, { astFactory: r }) => {
			r.printFilter(n, () => t("("));
			for (let [i, a] of n.triples.entries()) i % 2 == 0 && (r.printFilter(n, () => t("")), e(gC, a.object));
			r.printFilter(n, () => t(")"));
		}
	};
}
var oC = aC("collection", !1), sC = aC("collectionPath", !0);
function cC(e, t) {
	return {
		name: e,
		impl: ({ SUBRULE: e, OR: n }) => () => n([{ ALT: () => e(t ? sC : oC) }, { ALT: () => e(t ? pC : fC) }]),
		gImpl: ({ SUBRULE: e }) => (t) => t.subType === "list" ? e(sC, t) : e(pC, t)
	};
}
var lC = cC("triplesNode", !1), uC = cC("triplesNodePath", !0);
function dC(e, t) {
	let n = t ? XS : YS;
	return {
		name: e,
		impl: ({ ACTION: e, SUBRULE: t, CONSUME: r }) => (i) => {
			let a = r(pv), o = e(() => i.astFactory.termBlank(void 0, i.astFactory.sourceLocation())), s = t(n, o), c = r(mv);
			return e(() => i.astFactory.tripleCollectionBlankNodeProperties(o, s, i.astFactory.sourceLocation(a, c)));
		},
		gImpl: ({ SUBRULE: e, PRINT: t, PRINT_WORD: n, HANDLE_LOC: r, PRINT_ON_EMPTY: i, NEW_LINE: a }) => (o, s) => {
			let { astFactory: c, indentInc: l } = s;
			c.printFilter(o, () => {
				s[V] += l, t("["), a();
			});
			for (let t of o.triples) r(t, () => {
				c.isTerm(t.predicate) && c.isTermVariable(t.predicate) ? e(Xb, t.predicate) : e(kS, t.predicate, void 0), c.printFilter(t, () => n("")), e(gC, t.object), c.printFilter(o, () => {
					n(";"), a();
				});
			});
			c.printFilter(o, () => {
				s[V] -= l, i("]");
			});
		}
	};
}
var fC = dC("blankNodePropertyList", !1), pC = dC("blankNodePropertyListPath", !0);
function mC(e, t) {
	let n = t ? uC : lC;
	return {
		name: e,
		impl: ({ SUBRULE: e, OR: t }) => (r) => t([{ ALT: () => e(Xb) }, {
			GATE: () => r.parseMode.has("canCreateBlankNodes"),
			ALT: () => e(n)
		}]),
		gImpl: ({ SUBRULE: e }) => (t, { astFactory: r }) => {
			r.isTerm(t) ? e(Xb, t) : e(n, t);
		}
	};
}
var hC = mC("graphNode", !1), gC = mC("graphNodePath", !0), _C = {
	name: "whereClause",
	impl: ({ ACTION: e, SUBRULE: t, CONSUME: n, OPTION: r }) => (i) => {
		let a = r(() => n(my)), o = t(Q);
		return e(() => i.astFactory.wrap(o, i.astFactory.sourceLocation(a, o)));
	},
	gImpl: ({ SUBRULE: e, PRINT_WORD: t }) => (n, { astFactory: r }) => {
		r.printFilter(n, () => t("WHERE")), e(Q, n.val);
	}
}, Q = {
	name: "groupGraphPattern",
	impl: ({ ACTION: e, SUBRULE: t, CONSUME: n, OR: r }) => (i) => {
		let a = n(cv), o = r([{ ALT: () => [t(dw)] }, { ALT: () => t(yC) }]), s = n(lv);
		return e(() => !i.skipValidation && Mb(o)), e(() => i.astFactory.patternGroup(o, i.astFactory.sourceLocation(a, s)));
	},
	gImpl: ({ SUBRULE: e, PRINT_WORD: t, NEW_LINE: n, PRINT_ON_OWN_LINE: r }) => (i, a) => {
		let { astFactory: o, indentInc: s } = a;
		o.printFilter(i, () => {
			a[V] += s, t("{"), n();
		});
		for (let t of i.patterns) e(vC, t);
		o.printFilter(i, () => {
			a[V] -= s, r("}");
		});
	}
}, vC = {
	name: "generatePattern",
	gImpl: ({ SUBRULE: e }) => (t, { astFactory: n }) => {
		t.type === "query" ? e(lw, n.querySelect({
			context: [],
			datasets: n.datasetClauses([], n.sourceLocation()),
			where: t.where,
			variables: t.variables,
			solutionModifiers: t.solutionModifiers,
			values: t.values,
			distinct: t.distinct,
			reduced: t.reduced
		}, t.loc)) : t.subType === "group" ? e(Q, t) : t.subType === "bgp" ? e(BS, t) : e(bC, t);
	}
}, yC = {
	name: "groupGraphPatternSub",
	impl: ({ ACTION: e, SUBRULE: t, CONSUME: n, MANY: r, SUBRULE1: i, SUBRULE2: a, OPTION1: o, OPTION2: s, OPTION3: c }) => (l) => {
		let u = [], d = o(() => i(BS));
		return d && u.push(d), r(() => {
			let e = t(bC);
			u.push(e), s(() => n(uv));
			let r = c(() => a(BS));
			r && u.push(r);
		}), e(() => !l.skipValidation && Ab(u)), u;
	}
}, bC = {
	name: "graphPatternNotTriples",
	impl: ({ SUBRULE: e, OR: t }) => () => t([
		{ ALT: () => e(jC) },
		{ ALT: () => e(xC) },
		{ ALT: () => e(AC) },
		{ ALT: () => e(SC) },
		{ ALT: () => e(CC) },
		{ ALT: () => e(MC) },
		{ ALT: () => e(wC) },
		{ ALT: () => e(TC) }
	]),
	gImpl: ({ SUBRULE: e }) => (t) => {
		switch (t.subType) {
			case "group":
			case "union":
				e(jC, t);
				break;
			case "optional":
				e(xC, t);
				break;
			case "minus":
				e(AC, t);
				break;
			case "graph":
				e(SC, t);
				break;
			case "service":
				e(CC, t);
				break;
			case "filter":
				e(MC, t);
				break;
			case "bind":
				e(wC, t);
				break;
			case "values": e(TC, t);
		}
	}
}, xC = {
	name: "optionalGraphPattern",
	impl: ({ ACTION: e, SUBRULE: t, CONSUME: n }) => (r) => {
		let i = n(Vy), a = t(Q);
		return e(() => r.astFactory.patternOptional(a.patterns, r.astFactory.sourceLocation(i, a)));
	},
	gImpl: ({ SUBRULE: e, PRINT_WORD: t }) => (n, { astFactory: r }) => {
		r.printFilter(n, () => t("OPTIONAL")), e(Q, r.patternGroup(n.patterns, n.loc));
	}
}, SC = {
	name: "graphGraphPattern",
	impl: ({ ACTION: e, SUBRULE: t, CONSUME: n }) => (r) => {
		let i = n(T_), a = t(Zb), o = t(Q);
		return e(() => r.astFactory.patternGraph(a, o.patterns, r.astFactory.sourceLocation(i, o)));
	},
	gImpl: ({ SUBRULE: e, PRINT_WORD: t }) => (n, { astFactory: r }) => {
		r.printFilter(n, () => t("GRAPH")), e(Xb, n.name), e(Q, r.patternGroup(n.patterns, n.loc));
	}
}, CC = {
	name: "serviceGraphPattern",
	impl: ({ ACTION: e, SUBRULE1: t, CONSUME: n, OPTION: r }) => (i) => {
		let a = n(Hy), o = r(() => (n(Ty), !0)) ?? !1, s = t(Zb), c = t(Q);
		return e(() => i.astFactory.patternService(s, c.patterns, o, i.astFactory.sourceLocation(a, c)));
	},
	gImpl: ({ SUBRULE: e, PRINT_WORD: t }) => (n, { astFactory: r }) => {
		r.printFilter(n, () => {
			t("SERVICE"), n.silent && t("SILENT");
		}), e(Xb, n.name), e(Q, r.patternGroup(n.patterns, n.loc));
	}
}, wC = {
	name: "bind",
	impl: ({ ACTION: e, SUBRULE: t, CONSUME: n }) => (r) => {
		let i = n(Uy);
		n(W);
		let a = t($);
		n(ly);
		let o = t(X), s = n(G);
		return e(() => r.astFactory.patternBind(a, o, r.astFactory.sourceLocation(i, s)));
	},
	gImpl: ({ SUBRULE: e, PRINT_WORD: t, NEW_LINE: n }) => (r, { astFactory: i }) => {
		i.printFilter(r, () => t("BIND", "(")), e($, r.expression), i.printFilter(r, () => t("AS")), e(X, r.variable), i.printFilter(r, () => {
			t(")"), n();
		});
	}
}, TC = {
	name: "inlineData",
	impl: ({ ACTION: e, SUBRULE: t, CONSUME: n }) => (r) => {
		let i = n(Cy), a = t(EC);
		return e(() => (a.loc = r.astFactory.sourceLocation(i, a), a));
	},
	gImpl: ({ SUBRULE: e, PRINT_WORD: t, PRINT_ON_EMPTY: n, NEW_LINE: r, PRINT_ON_OWN_LINE: i }) => (a, o) => {
		let { astFactory: s, indentInc: c } = o, l = a.variables, u = l.length === 1;
		s.printFilter(a, () => {
			n("VALUES", u ? "" : "( ");
		});
		for (let n of l) s.printFilter(a, () => t("")), e(Xb, n), s.printFilter(a, () => t(""));
		s.printFilter(a, () => {
			o[V] += c, t(u ? "" : ")", "{"), r();
		});
		for (let n of a.values) {
			s.printFilter(a, () => !u && t("("));
			for (let r of l) {
				let i = r.value;
				n[i] === void 0 ? s.printFilter(a, () => t("UNDEF")) : (e(gC, n[i]), s.printFilter(a, () => t("")));
			}
			s.printFilter(a, () => {
				t(u ? "" : ")"), r();
			});
		}
		s.printFilter(a, () => {
			o[V] -= c, i("}");
		});
	}
}, EC = {
	name: "dataBlock",
	impl: ({ SUBRULE: e, OR: t }) => () => t([{ ALT: () => e(DC) }, { ALT: () => e(OC) }])
}, DC = {
	name: "inlineDataOneVar",
	impl: ({ ACTION: e, SUBRULE: t, CONSUME: n, MANY: r }) => (i) => {
		let a = [], o = t(X);
		n(cv), r(() => {
			let n = t(kC);
			e(() => {
				a.push(Object.assign(Object.create(null), { [o.value]: n }));
			});
		});
		let s = n(lv);
		return e(() => i.astFactory.patternValues([o], a, i.astFactory.sourceLocation(o, s)));
	}
}, OC = {
	name: "inlineDataFull",
	impl: ({ ACTION: e, OR: t, MANY1: n, MANY2: r, MANY3: i, MANY4: a, SUBRULE: o, CONSUME1: s, CONSUME2: c }) => (l) => {
		let u = [], d = [];
		return t([{ ALT: () => {
			let t = s(ty);
			s(cv), n(() => {
				c(ty), u.push(Object.create(null));
			});
			let r = s(lv);
			return e(() => l.astFactory.patternValues(d, u, l.astFactory.sourceLocation(t, r)));
		} }, { ALT: () => {
			let t = s(W);
			r(() => {
				d.push(o(X));
			}), s(G), c(cv), i(() => {
				let t = 0, n = Object.create(null);
				c(W), a(() => {
					e(() => {
						if (!l.skipValidation && t >= d.length) throw Error("Number of dataBlockValues does not match number of variables. Too much values.");
					});
					let r = o(kC);
					e(() => {
						n[d[t].value] = r, t++;
					});
				}), c(G), e(() => {
					if (u.push(n), !l.skipValidation && d.length !== t) throw Error("Number of dataBlockValues does not match number of variables. Too few values.");
				});
			});
			let n = c(lv);
			return e(() => l.astFactory.patternValues(d, u, l.astFactory.sourceLocation(t, n)));
		} }]);
	}
}, kC = {
	name: "dataBlockValue",
	impl: ({ SUBRULE: e, CONSUME: t, OR: n }) => () => n([
		{ ALT: () => e(Y) },
		{ ALT: () => e(Fb) },
		{ ALT: () => e(Ib) },
		{ ALT: () => e(Bb) },
		{ ALT: () => {
			t(Wy);
		} }
	])
}, AC = {
	name: "minusGraphPattern",
	impl: ({ ACTION: e, SUBRULE: t, CONSUME: n }) => (r) => {
		let i = n(Gy), a = t(Q);
		return e(() => r.astFactory.patternMinus(a.patterns, r.astFactory.sourceLocation(i, a)));
	},
	gImpl: ({ SUBRULE: e, PRINT_WORD: t }) => (n, { astFactory: r }) => {
		r.printFilter(n, () => t("MINUS")), e(Q, r.patternGroup(n.patterns, n.loc));
	}
}, jC = {
	name: "groupOrUnionGraphPattern",
	impl: ({ ACTION: e, MANY: t, SUBRULE1: n, SUBRULE2: r, CONSUME: i }) => (a) => {
		let o = [], s = n(Q);
		return o.push(s), t(() => {
			i(Ky);
			let e = r(Q);
			o.push(e);
		}), e(() => o.length === 1 ? o[0] : a.astFactory.patternUnion(o, a.astFactory.sourceLocation(s, o.at(-1))));
	},
	gImpl: ({ SUBRULE: e, PRINT_WORD: t }) => (n, { astFactory: r }) => {
		if (r.isPatternUnion(n)) {
			let [i, ...a] = n.patterns;
			e(Q, i);
			for (let i of a) r.printFilter(n, () => t("UNION")), e(Q, i);
		} else e(Q, n);
	}
}, MC = {
	name: "filter",
	impl: ({ ACTION: e, SUBRULE: t, CONSUME: n }) => (r) => {
		let i = n(qy), a = t(NC);
		return e(() => r.astFactory.patternFilter(a, r.astFactory.sourceLocation(i, a)));
	},
	gImpl: ({ SUBRULE: e, PRINT_WORD: t, NEW_LINE: n }) => (r, { astFactory: i }) => {
		i.printFilter(r, () => t("FILTER (")), e($, r.expression), i.printFilter(r, () => {
			t(")"), n();
		});
	}
}, NC = {
	name: "constraint",
	impl: ({ SUBRULE: e, OR: t }) => () => t([
		{ ALT: () => e(XC) },
		{ ALT: () => e(mS) },
		{ ALT: () => e(PC) }
	])
}, PC = {
	name: "functionCall",
	impl: ({ ACTION: e, SUBRULE: t }) => (n) => {
		let r = t(Y), i = t(FC);
		return e(() => n.astFactory.expressionFunctionCall(r, i.val.args, i.val.distinct, n.astFactory.sourceLocation(r, i)));
	}
}, FC = {
	name: "argList",
	impl: ({ ACTION: e, CONSUME: t, SUBRULE1: n, OPTION: r, OR: i, AT_LEAST_ONE_SEP: a }) => (o) => i([{ ALT: () => {
		let n = t(ty);
		return e(() => o.astFactory.wrap({
			args: [],
			distinct: !1
		}, o.astFactory.sourceLocation(n)));
	} }, { ALT: () => {
		let i = [], s = t(W), c = r(() => (t(sy), !0)) ?? !1;
		a({
			SEP: dv,
			DEF: () => {
				let e = n($);
				i.push(e);
			}
		});
		let l = t(G);
		return e(() => o.astFactory.wrap({
			args: i,
			distinct: c
		}, o.astFactory.sourceLocation(s, l)));
	} }]),
	gImpl: ({ SUBRULE: e, PRINT_WORD: t }) => (n, { astFactory: r }) => {
		r.printFilter(n, () => {
			t("("), n.val.distinct && t("DISTINCT");
		});
		let [i, ...a] = n.val.args;
		i && e($, i);
		for (let i of a) r.printFilter(n, () => t(",")), e($, i);
		r.printFilter(n, () => t(")"));
	}
}, IC = {
	name: "expressionList",
	impl: ({ ACTION: e, CONSUME: t, MANY: n, OR: r, SUBRULE1: i, SUBRULE2: a }) => (o) => r([{ ALT: () => {
		let n = t(ty);
		return e(() => o.astFactory.wrap([], o.astFactory.sourceLocation(n)));
	} }, { ALT: () => {
		let r = t(W), s = [i($)];
		n(() => {
			t(dv);
			let e = a($);
			s.push(e);
		});
		let c = t(G);
		return e(() => o.astFactory.wrap(s, o.astFactory.sourceLocation(r, c)));
	} }])
}, LC = /* @__PURE__ */ new Set([
	"||",
	"&&",
	"=",
	"!=",
	"<",
	">",
	"<=",
	">=",
	"+",
	"-",
	"*",
	"/"
]), RC = /* @__PURE__ */ new Set([
	"in",
	"notin",
	"||",
	"&&",
	"=",
	"!=",
	"<",
	">",
	"<=",
	">=",
	"+",
	"-",
	"*",
	"/"
]), zC = {
	"!": "",
	uplus: "+",
	uminus: "-"
}, $ = {
	name: "expression",
	impl: ({ SUBRULE: e }) => () => e(VC),
	gImpl: ({ SUBRULE: e, PRINT_WORD: t }) => (n, { astFactory: r }) => {
		if (r.isTerm(n)) e(Xb, n);
		else if (r.isExpressionPatternOperation(n)) {
			let i = n.args;
			r.printFilter(n, () => t(n.operator === "exists" ? "EXISTS" : "NOT EXISTS")), e(Q, i);
		} else if (r.isExpressionFunctionCall(n)) e(ZC, n);
		else if (r.isExpressionAggregate(n)) e(DS, n);
		else if (RC.has(n.operator)) {
			let [i, ...a] = n.args;
			r.printFilter(n, () => t("(")), e($, i), r.printFilter(n, () => {
				n.operator === "notin" ? t("NOT IN") : n.operator === "in" ? t("IN") : t(n.operator.toUpperCase());
			}), a.length === 1 && LC.has(n.operator) ? e($, a[0]) : e(FC, r.wrap({
				args: a,
				distinct: !1
			}, n.loc)), r.printFilter(n, () => t(")"));
		} else if (typeof zC[n.operator] == "string") {
			let [i] = n.args;
			r.printFilter(n, () => t(zC[n.operator] || n.operator.toUpperCase())), e($, i);
		} else {
			r.printFilter(n, () => t(n.operator.toUpperCase(), "("));
			let [i, ...a] = n.args;
			i && e($, i);
			for (let i of a) r.printFilter(n, () => t(",")), e($, i);
			r.printFilter(n, () => t(")"));
		}
	}
};
function BC(e, t, n, r) {
	let i = e();
	return r(() => {
		let e = t();
		n(() => {
			i = e(i);
		});
	}), i;
}
var VC = {
	name: "conditionalOrExpression",
	impl: ({ ACTION: e, MANY: t, CONSUME: n, SUBRULE1: r, SUBRULE2: i }) => (a) => BC(() => r(HC), () => {
		n(wv);
		let t = i(HC);
		return (n) => e(() => a.astFactory.expressionOperation("||", [n, t], a.astFactory.sourceLocation(n, t)));
	}, e, t)
}, HC = {
	name: "conditionalAndExpression",
	impl: ({ ACTION: e, MANY: t, SUBRULE1: n, SUBRULE2: r, CONSUME: i }) => (a) => BC(() => n(UC), () => {
		i(Cv);
		let t = r(UC);
		return (n) => e(() => a.astFactory.expressionOperation("&&", [n, t], a.astFactory.sourceLocation(n, t)));
	}, e, t)
}, UC = {
	name: "valueLogical",
	impl: ({ SUBRULE: e }) => () => e(WC)
}, WC = {
	name: "relationalExpression",
	impl: ({ ACTION: e, CONSUME: t, SUBRULE1: n, SUBRULE2: r, OPTION: i, OR1: a, OR2: o, OR3: s }) => (c) => {
		let l = n(GC);
		return i(() => a([{ ALT: () => {
			let n = o([
				{ ALT: () => t(Tv) },
				{ ALT: () => t(Ev) },
				{ ALT: () => t(Dv) },
				{ ALT: () => t(Ov) },
				{ ALT: () => t(kv) },
				{ ALT: () => t(Av) }
			]), i = r(GC);
			return e(() => c.astFactory.expressionOperation(n.image, [l, i], c.astFactory.sourceLocation(l, i)));
		} }, { ALT: () => {
			let r = s([{ ALT: () => t(Zy) }, { ALT: () => t(Qy) }]), i = n(IC);
			return e(() => c.astFactory.expressionOperation(r.image, [l, ...i.val], c.astFactory.sourceLocation(l, i)));
		} }])) ?? l;
	}
}, GC = {
	name: "numericExpression",
	impl: ({ SUBRULE: e }) => () => e(KC)
}, KC = {
	name: "additiveExpression",
	impl: ({ ACTION: e, SUBRULE: t, CONSUME: n, SUBRULE1: r, SUBRULE2: i, MANY1: a, MANY2: o, OR1: s, OR2: c, OR3: l, OR4: u }) => (d) => BC(() => r(qC), () => s([{ ALT: () => {
		let t = c([{ ALT: () => n(bv) }, { ALT: () => n(xv) }]), r = i(qC);
		return e(() => (e) => d.astFactory.expressionOperation(t.image, [e, r], d.astFactory.sourceLocation(e, r)));
	} }, { ALT: () => {
		let { operator: i, startInt: a } = l([{ ALT: () => {
			let n = t(Rb);
			return e(() => (n.value = n.value.replace(/^\+/u, ""), {
				operator: "+",
				startInt: n
			}));
		} }, { ALT: () => {
			let n = t(zb);
			return e(() => (n.value = n.value.replace(/^-/u, ""), {
				operator: "-",
				startInt: n
			}));
		} }]), s = BC(() => e(() => a), () => {
			let t = u([{ ALT: () => n(yv) }, { ALT: () => n(gv) }]), i = r(JC);
			return (n) => e(() => d.astFactory.expressionOperation(t.image, [n, i], d.astFactory.sourceLocation(n, i)));
		}, e, o);
		return (e) => d.astFactory.expressionOperation(i, [e, s], d.astFactory.sourceLocation(e, s));
	} }]), e, a)
}, qC = {
	name: "multiplicativeExpression",
	impl: ({ ACTION: e, CONSUME: t, MANY: n, SUBRULE1: r, SUBRULE2: i, OR: a }) => (o) => BC(() => r(JC), () => {
		let e = a([{ ALT: () => t(yv) }, { ALT: () => t(gv) }]), n = i(JC);
		return (t) => ({
			type: "expression",
			subType: "operation",
			operator: e.image,
			args: [t, n],
			loc: o.astFactory.sourceLocation(t, n)
		});
	}, e, n)
}, JC = {
	name: "unaryExpression",
	impl: ({ ACTION: e, CONSUME: t, SUBRULE1: n, SUBRULE2: r, OR1: i, OR2: a }) => (o) => i([{ ALT: () => n(YC) }, { ALT: () => {
		let n = a([
			{ ALT: () => t(Sv) },
			{ ALT: () => t(bv) },
			{ ALT: () => t(xv) }
		]), i = r(YC);
		return e(() => o.astFactory.expressionOperation(n.image === "!" ? "!" : n.image === "+" ? "UPLUS" : "UMINUS", [i], o.astFactory.sourceLocation(n, i)));
	} }])
}, YC = {
	name: "primaryExpression",
	impl: ({ SUBRULE: e, OR: t }) => () => t([
		{ ALT: () => e(XC) },
		{ ALT: () => e(mS) },
		{ ALT: () => e(ZC) },
		{ ALT: () => e(Fb) },
		{ ALT: () => e(Ib) },
		{ ALT: () => e(Bb) },
		{ ALT: () => e(X) }
	])
}, XC = {
	name: "brackettedExpression",
	impl: ({ ACTION: e, SUBRULE: t, CONSUME: n }) => (r) => {
		let i = n(W), a = t($), o = n(G);
		return e(() => (a.loc = r.astFactory.sourceLocation(i, o), a));
	}
}, ZC = {
	name: "iriOrFunction",
	impl: ({ ACTION: e, SUBRULE: t, OPTION: n }) => (r) => {
		let i = t(Y);
		return n(() => {
			let n = t(FC);
			return e(() => {
				let e = n.val.distinct;
				if (!r.parseMode.has("canParseAggregate") && e) throw Error("DISTINCT implies that this function is an aggregated function, which is not allowed in this context.");
				return {
					type: "expression",
					subType: "functionCall",
					function: i,
					args: n.val.args,
					distinct: e,
					loc: r.astFactory.sourceLocation(i, n)
				};
			});
		}) ?? i;
	},
	gImpl: ({ SUBRULE: e }) => (t, { astFactory: n }) => {
		n.isTermNamed(t) ? e(Y, t) : (e(Y, t.function), e(FC, n.wrap({
			args: t.args,
			distinct: t.distinct
		}, t.loc)));
	}
}, QC = {
	name: "solutionModifier",
	impl: ({ ACTION: e, SUBRULE: t, OPTION1: n, OPTION2: r, OPTION3: i, OPTION4: a }) => () => {
		let o = n(() => t($C)), s = r(() => t(tw)), c = i(() => t(rw)), l = a(() => t(aw));
		return e(() => ({
			...l && { limitOffset: l },
			...o && { group: o },
			...s && { having: s },
			...c && { order: c }
		}));
	},
	gImpl: ({ SUBRULE: e }) => (t) => {
		t.group && e($C, t.group), t.having && e(tw, t.having), t.order && e(rw, t.order), t.limitOffset && e(aw, t.limitOffset);
	}
}, $C = {
	name: "groupClause",
	impl: ({ ACTION: e, AT_LEAST_ONE: t, SUBRULE1: n, CONSUME: r }) => (i) => {
		let a = [], o = r(hy);
		return r(gy), t(() => {
			a.push(n(ew));
		}), e(() => ({
			type: "solutionModifier",
			subType: "group",
			groupings: a,
			loc: i.astFactory.sourceLocation(o, a.at(-1))
		}));
	},
	gImpl: ({ PRINT_WORDS: e, SUBRULE: t, PRINT_ON_EMPTY: n }) => (r, { astFactory: i }) => {
		i.printFilter(r, () => {
			n("GROUP BY ");
		});
		for (let n of r.groupings) i.printFilter(r, () => e("")), i.isExpression(n) ? t($, n) : (i.printFilter(r, () => e("(")), t($, n.value), i.printFilter(r, () => e("AS")), t(X, n.variable), i.printFilter(r, () => e(")")));
	}
}, ew = {
	name: "groupCondition",
	impl: ({ ACTION: e, SUBRULE: t, CONSUME: n, SUBRULE1: r, SUBRULE2: i, OPTION: a, OR: o }) => (s) => o([
		{ ALT: () => t(mS) },
		{ ALT: () => t(PC) },
		{ ALT: () => i(X) },
		{ ALT: () => {
			let i = n(W), o = t($), c = a(() => (n(ly), r(X))), l = n(G);
			return e(() => c === void 0 ? o : {
				variable: c,
				value: o,
				loc: s.astFactory.sourceLocation(i, l)
			});
		} }
	])
}, tw = {
	name: "havingClause",
	impl: ({ ACTION: e, AT_LEAST_ONE: t, SUBRULE: n, CONSUME: r }) => (i) => {
		let a = r(_y), o = [], s = e(() => i.parseMode.has("canParseAggregate") || !i.parseMode.add("canParseAggregate"));
		return t(() => {
			o.push(n(nw));
		}), e(() => !s && i.parseMode.delete("canParseAggregate")), e(() => i.astFactory.solutionModifierHaving(o, i.astFactory.sourceLocation(a, o.at(-1))));
	},
	gImpl: ({ PRINT_ON_EMPTY: e, SUBRULE: t }) => (n, { astFactory: r }) => {
		r.printFilter(n, () => {
			e("HAVING ");
		});
		for (let e of n.having) t($, e);
	}
}, nw = {
	name: "havingCondition",
	impl: ({ SUBRULE: e }) => () => e(NC)
}, rw = {
	name: "orderClause",
	impl: ({ ACTION: e, AT_LEAST_ONE: t, SUBRULE1: n, CONSUME: r }) => (i) => {
		let a = r(vy);
		r(gy);
		let o = [], s = e(() => i.parseMode.has("canParseAggregate") || !i.parseMode.add("canParseAggregate"));
		return t(() => {
			o.push(n(iw));
		}), e(() => !s && i.parseMode.delete("canParseAggregate")), e(() => i.astFactory.solutionModifierOrder(o, i.astFactory.sourceLocation(a, o.at(-1))));
	},
	gImpl: ({ PRINT_WORDS: e, PRINT_ON_EMPTY: t, SUBRULE: n }) => (r, { astFactory: i }) => {
		i.printFilter(r, () => {
			t("ORDER BY ");
		});
		for (let t of r.orderDefs) t.descending ? i.printFilter(r, () => e("DESC")) : i.printFilter(r, () => e("ASC")), i.printFilter(r, () => e("(")), n($, t.expression), i.printFilter(r, () => e(")"));
	}
}, iw = {
	name: "orderCondition",
	impl: ({ ACTION: e, SUBRULE: t, CONSUME: n, OR1: r, OR2: i }) => (a) => r([
		{ ALT: () => {
			let r = i([{ ALT: () => [!1, n(yy)] }, { ALT: () => [!0, n(by)] }]), o = t(XC);
			return e(() => ({
				expression: o,
				descending: r[0],
				loc: a.astFactory.sourceLocation(r[1], o)
			}));
		} },
		{ ALT: () => {
			let n = t(NC);
			return e(() => ({
				expression: n,
				descending: !1,
				loc: n.loc
			}));
		} },
		{ ALT: () => {
			let n = t(X);
			return e(() => ({
				expression: n,
				descending: !1,
				loc: n.loc
			}));
		} }
	])
}, aw = {
	name: "limitOffsetClauses",
	impl: ({ ACTION: e, SUBRULE1: t, SUBRULE2: n, OPTION1: r, OPTION2: i, OR: a }) => (o) => a([{ ALT: () => {
		let n = t(ow), i = r(() => t(sw));
		return e(() => o.astFactory.solutionModifierLimitOffset(n.val, i?.val, o.astFactory.sourceLocation(n, ...i ? [i] : [])));
	} }, { ALT: () => {
		let t = n(sw), r = i(() => n(ow));
		return e(() => o.astFactory.solutionModifierLimitOffset(r?.val, t.val, o.astFactory.sourceLocation(t, r)));
	} }]),
	gImpl: ({ PRINT_WORDS: e, NEW_LINE: t }) => (n, { astFactory: r }) => {
		r.printFilter(n, () => {
			t(), n.limit !== void 0 && e("LIMIT", String(n.limit)), n.offset && e("OFFSET", String(n.offset));
		});
	}
}, ow = {
	name: "limitClause",
	impl: ({ ACTION: e, CONSUME: t }) => (n) => {
		let r = t(xy), i = t(Bv), a = Number.parseInt(i.image, 10);
		return e(() => n.astFactory.wrap(a, n.astFactory.sourceLocation(r, i)));
	}
}, sw = {
	name: "offsetClause",
	impl: ({ CONSUME: e, ACTION: t }) => (n) => {
		let r = e(Sy), i = e(Bv), a = Number.parseInt(i.image, 10);
		return t(() => n.astFactory.wrap(a, n.astFactory.sourceLocation(r, i)));
	}
}, cw = {
	name: "queryUnit",
	impl: ({ SUBRULE: e }) => () => e(lw)
}, lw = {
	name: "query",
	impl: ({ ACTION: e, SUBRULE: t, OR: n }) => (r) => {
		let i = t(Kb), a = n([
			{ ALT: () => t(uw) },
			{ ALT: () => t(pw) },
			{ ALT: () => t(mw) },
			{ ALT: () => t(hw) }
		]), o = t(gw);
		return e(() => {
			let e = {
				context: i,
				...a,
				type: "query",
				loc: r.astFactory.sourceLocation(i.at(0), a, o)
			};
			return o && (e.values = o), e;
		});
	},
	gImpl: ({ SUBRULE: e }) => (t, { astFactory: n }) => {
		e(Kb, t.context), n.isQuerySelect(t) ? e(uw, t) : n.isQueryConstruct(t) ? e(pw, t) : n.isQueryDescribe(t) ? e(mw, t) : e(hw, t), t.values && e(TC, t.values);
	}
}, uw = {
	name: "selectQuery",
	impl: ({ ACTION: e, SUBRULE: t }) => (n) => {
		let r = t(fw), i = t(ix), a = t(_C), o = t(QC);
		return e(() => {
			let e = {
				subType: "select",
				where: a.val,
				solutionModifiers: o,
				datasets: i,
				...r.val,
				loc: n.astFactory.sourceLocation(r, a, o.group, o.having, o.order, o.limitOffset)
			};
			return n.skipValidation || Ob(e), e;
		});
	},
	gImpl: ({ SUBRULE: e }) => (t, { astFactory: n }) => {
		e(fw, n.wrap({
			variables: t.variables,
			distinct: t.distinct,
			reduced: t.reduced
		}, n.sourceLocation(...t.variables))), e(ix, t.datasets), e(_C, n.wrap(t.where, t.where.loc)), e(QC, t.solutionModifiers);
	}
}, dw = {
	name: "subSelect",
	impl: ({ ACTION: e, SUBRULE: t }) => (n) => {
		let r = t(fw), i = t(_C), a = t(QC), o = t(gw);
		return e(() => n.astFactory.querySelect({
			where: i.val,
			datasets: n.astFactory.datasetClauses([], n.astFactory.sourceLocation()),
			context: [],
			solutionModifiers: a,
			...r.val,
			...o && { values: o }
		}, n.astFactory.sourceLocation(r, i, a.group, a.having, a.order, a.limitOffset, o)));
	}
}, fw = {
	name: "selectClause",
	impl: ({ ACTION: e, AT_LEAST_ONE: t, SUBRULE1: n, SUBRULE2: r, CONSUME: i, OPTION: a, OR1: o, OR2: s, OR3: c }) => (l) => {
		let u = i(oy), d = e(() => l.parseMode.has("canParseAggregate") || !l.parseMode.add("canParseAggregate")), f = a(() => o([{ ALT: () => (i(sy), [!0, !1]) }, { ALT: () => (i(cy), [!1, !0]) }])) ?? [!1, !1], p = e(() => {
			let [e, t] = f;
			return {
				...e && { distinct: e },
				...t && { reduced: t }
			};
		}), m, h = s([{ ALT: () => {
			let t = i(yv);
			return e(() => (m = t, {
				variables: [l.astFactory.wildcard(l.astFactory.sourceLocation(t))],
				...p
			}));
		} }, { ALT: () => {
			let a = [], o = [];
			return t(() => c([{ ALT: () => {
				let t = n(X);
				e(() => {
					if (!l.skipValidation && a.some((e) => e.value === t.value)) throw Error(`Variable ${t.value} used more than once in SELECT clause`);
					a.push(t), o.push(t), m = t;
				});
			} }, { ALT: () => {
				let t = i(W), s = n($);
				i(ly);
				let c = r(X), u = i(G);
				e(() => {
					if (m = u, !l.skipValidation && a.some((e) => e.value === c.value)) throw Error(`Variable ${c.value} used more than once in SELECT clause`);
					a.push(c), o.push(l.astFactory.patternBind(s, c, l.astFactory.sourceLocation(t, m)));
				});
			} }])), {
				variables: o,
				...p
			};
		} }]);
		return e(() => !d && l.parseMode.delete("canParseAggregate")), e(() => l.astFactory.wrap(h, l.astFactory.sourceLocation(u, m)));
	},
	gImpl: ({ SUBRULE: e, PRINT_WORD: t, PRINT_ON_EMPTY: n }) => (r, { astFactory: i }) => {
		i.printFilter(r, () => {
			n("SELECT "), r.val.distinct ? t("DISTINCT") : r.val.reduced && t("REDUCED");
		});
		for (let n of r.val.variables) i.isWildcard(n) ? i.printFilter(r, () => t("*")) : i.isTerm(n) ? e(X, n) : (i.printFilter(r, () => t("(")), e($, n.expression), i.printFilter(r, () => t("AS")), e(X, n.variable), i.printFilter(r, () => t(")"))), i.printFilter(r, () => t(""));
		i.printFilter(r, () => t(""));
	}
}, pw = {
	name: "constructQuery",
	impl: ({ ACTION: e, SUBRULE1: t, SUBRULE2: n, CONSUME: r, OR: i }) => (a) => {
		let o = r(uy);
		return i([{ ALT: () => {
			let n = t(_w), r = t(ix), i = t(_C), s = t(QC);
			return e(() => ({
				subType: "construct",
				template: n.val,
				datasets: r,
				where: i.val,
				solutionModifiers: s,
				loc: a.astFactory.sourceLocation(o, i, s.group, s.having, s.order, s.limitOffset)
			}));
		} }, { ALT: () => {
			let t = n(ix);
			r(my);
			let i = n(_w), s = n(QC);
			return e(() => ({
				subType: "construct",
				template: i.val,
				datasets: t,
				where: a.astFactory.patternGroup([i.val], a.astFactory.sourceLocation()),
				solutionModifiers: s,
				loc: a.astFactory.sourceLocation(o, i, s.group, s.having, s.order, s.limitOffset)
			}));
		} }]);
	},
	gImpl: ({ SUBRULE: e, PRINT_WORD: t, PRINT_ON_EMPTY: n, PRINT_ON_OWN_LINE: r, NEW_LINE: i }) => (a, o) => {
		let { astFactory: s, indentInc: c } = o;
		s.printFilter(a, () => n("CONSTRUCT ")), s.isSourceLocationNoMaterialize(a.where.loc) || (s.printFilter(a, () => {
			o[V] += c, t("{"), i();
		}), e(BS, a.template), s.printFilter(a, () => {
			o[V] -= c, r("}");
		})), e(ix, a.datasets), s.isSourceLocationNoMaterialize(a.where.loc) ? e(_C, s.wrap(s.patternGroup([a.template], a.template.loc), a.template.loc)) : e(_C, s.wrap(a.where, a.where.loc)), e(QC, a.solutionModifiers);
	}
}, mw = {
	name: "describeQuery",
	impl: ({ ACTION: e, AT_LEAST_ONE: t, SUBRULE1: n, CONSUME: r, OPTION: i, OR: a }) => (o) => {
		let s = r(dy), c = a([{ ALT: () => {
			let e = [];
			return t(() => {
				e.push(n(Zb));
			}), e;
		} }, { ALT: () => {
			let t = r(yv);
			return [e(() => o.astFactory.wildcard(o.astFactory.sourceLocation(t)))];
		} }]), l = n(ix), u = i(() => n(_C)), d = n(QC);
		return e(() => ({
			subType: "describe",
			variables: c,
			datasets: l,
			...u && { where: u.val },
			solutionModifiers: d,
			loc: o.astFactory.sourceLocation(s, ...c, l, u, d.group, d.having, d.order, d.limitOffset)
		}));
	},
	gImpl: ({ SUBRULE: e, PRINT_WORD: t, PRINT_ON_EMPTY: n }) => (r, { astFactory: i }) => {
		if (i.printFilter(r, () => n("DESCRIBE ")), i.isWildcard(r.variables[0])) i.printFilter(r, () => t("*"));
		else for (let n of r.variables) i.printFilter(r, () => t("")), e(Xb, n);
		e(ix, r.datasets), r.where && e(_C, i.wrap(r.where, r.loc)), e(QC, r.solutionModifiers);
	}
}, hw = {
	name: "askQuery",
	impl: ({ ACTION: e, SUBRULE: t, CONSUME: n }) => (r) => {
		let i = n(fy), a = t(ix), o = t(_C), s = t(QC);
		return e(() => ({
			subType: "ask",
			datasets: a,
			where: o.val,
			solutionModifiers: s,
			loc: r.astFactory.sourceLocation(i, a, o, s.group, s.having, s.order, s.limitOffset)
		}));
	},
	gImpl: ({ SUBRULE: e, PRINT_ON_EMPTY: t }) => (n, { astFactory: r }) => {
		r.printFilter(n, () => t("ASK ")), e(ix, n.datasets), e(_C, r.wrap(n.where, n.loc)), e(QC, n.solutionModifiers);
	}
}, gw = {
	name: "valuesClause",
	impl: ({ OPTION: e, SUBRULE: t }) => () => e(() => t(TC))
}, _w = {
	name: "constructTemplate",
	impl: ({ ACTION: e, SUBRULE1: t, CONSUME: n, OPTION: r }) => (i) => {
		let a = n(cv), o = r(() => t(vw)), s = n(lv);
		return e(() => i.astFactory.wrap(o ?? i.astFactory.patternBgp([], i.astFactory.sourceLocation()), i.astFactory.sourceLocation(a, s)));
	}
}, vw = {
	name: "constructTriples",
	impl: WS.impl
}, yw = {
	name: "updateUnit",
	impl: ({ SUBRULE: e }) => () => e(bw)
}, bw = {
	name: "update",
	impl: ({ ACTION: e, SUBRULE: t, SUBRULE1: n, SUBRULE2: r, CONSUME: i, OPTION1: a, MANY: o }) => (s) => {
		let c = [], l = n(Kb);
		c.push({ context: l });
		let u = !0;
		return o({
			GATE: () => u,
			DEF: () => {
				u = !1, c.at(-1).operation = t(xw), a(() => {
					i(fv), u = !0;
					let e = r(Kb);
					c.push({ context: e });
				});
			}
		}), e(() => {
			let e = {
				type: "update",
				updates: c,
				loc: s.astFactory.sourceLocation(...c.flatMap((e) => [...e.context, e.operation]))
			};
			return s.skipValidation || jb(e), e;
		});
	},
	gImpl: ({ SUBRULE: e, PRINT: t, NEW_LINE: n }) => (r, { astFactory: i }) => {
		let [a, ...o] = r.updates;
		a && (e(Kb, a.context), a.operation && e(xw, a.operation));
		for (let a of o) i.printFilter(r, () => {
			t(";"), n();
		}), e(Kb, a.context), a.operation && e(xw, a.operation);
	}
}, xw = {
	name: "update1",
	impl: ({ SUBRULE: e, OR: t }) => () => t([
		{ ALT: () => e(Sw) },
		{ ALT: () => e(ww) },
		{ ALT: () => e(Tw) },
		{ ALT: () => e(Ow) },
		{ ALT: () => e(kw) },
		{ ALT: () => e(Aw) },
		{ ALT: () => e(Ew) },
		{ ALT: () => e(Pw) },
		{ ALT: () => e(Fw) },
		{ ALT: () => e(Iw) },
		{ ALT: () => e(Lw) }
	]),
	gImpl: ({ SUBRULE: e }) => (t) => {
		switch (t.subType) {
			case "load":
				e(Sw, t);
				break;
			case "clear":
				e(ww, t);
				break;
			case "drop":
				e(Tw, t);
				break;
			case "add":
				e(Ow, t);
				break;
			case "move":
				e(kw, t);
				break;
			case "copy":
				e(Aw, t);
				break;
			case "create":
				e(Ew, t);
				break;
			case "insertdata":
				e(Pw, t);
				break;
			case "deletedata":
				e(Fw, t);
				break;
			case "deletewhere":
				e(Iw, t);
				break;
			case "modify": e(Lw, t);
		}
	}
}, Sw = {
	name: "load",
	impl: ({ ACTION: e, SUBRULE1: t, CONSUME: n, OPTION1: r, OPTION2: i }) => (a) => {
		let o = n(wy), s = r(() => n(Ty)), c = t(Y), l = i(() => (n(Ey), t(Vw)));
		return e(() => a.astFactory.updateOperationLoad(a.astFactory.sourceLocation(o, c, l), c, !!s, l));
	},
	gImpl: ({ SUBRULE: e, PRINT_WORD: t, PRINT_ON_EMPTY: n }) => (r, { astFactory: i }) => {
		i.printFilter(r, () => {
			n("LOAD "), r.silent && t("SILENT");
		}), e(Y, r.source), r.destination && (i.printFilter(r, () => t("INTO")), e(Hw, r.destination));
	}
};
function Cw(e) {
	return {
		name: z(e.name),
		impl: ({ ACTION: t, SUBRULE1: n, CONSUME: r, OPTION: i }) => (a) => {
			let o = r(e), s = i(() => r(Ty)), c = n(Hw);
			return t(() => a.astFactory.updateOperationClearDrop(z(e.name), !!s, c, a.astFactory.sourceLocation(o, c)));
		},
		gImpl: ({ SUBRULE: t, PRINT_WORD: n, PRINT_ON_EMPTY: r }) => (i, { astFactory: a }) => {
			a.printFilter(i, () => {
				r(e.name.toUpperCase(), " "), i.silent && n("SILENT");
			}), t(Hw, i.destination);
		}
	};
}
var ww = Cw(Dy), Tw = Cw(Oy), Ew = {
	name: "create",
	impl: ({ ACTION: e, SUBRULE1: t, CONSUME: n, OPTION: r }) => (i) => {
		let a = n(ky), o = r(() => n(Ty)), s = t(Vw);
		return e(() => i.astFactory.updateOperationCreate(s, !!o, i.astFactory.sourceLocation(a, s)));
	},
	gImpl: ({ SUBRULE: e, PRINT_WORD: t, PRINT_ON_EMPTY: n }) => (r, { astFactory: i }) => {
		i.printFilter(r, () => {
			n("CREATE "), r.silent && t("SILENT");
		}), e(Hw, r.destination);
	}
};
function Dw(e) {
	return {
		name: z(e.name),
		impl: ({ ACTION: t, CONSUME: n, SUBRULE1: r, SUBRULE2: i, OPTION: a }) => (o) => {
			let s = n(e), c = a(() => n(Ty)), l = r(Bw);
			n(jy);
			let u = i(Bw);
			return t(() => o.astFactory.updateOperationAddMoveCopy(z(e.name), l, u, !!c, o.astFactory.sourceLocation(s, u)));
		},
		gImpl: ({ SUBRULE: t, PRINT_WORD: n, PRINT_ON_EMPTY: r }) => (i, { astFactory: a }) => {
			a.printFilter(i, () => {
				r(e.name.toUpperCase(), " "), i.silent && n("SILENT");
			}), t(Hw, i.source), a.printFilter(i, () => n("TO")), t(Hw, i.destination);
		}
	};
}
var Ow = Dw(Ay), kw = Dw(My), Aw = Dw(Ny), jw = {
	name: "quadPattern",
	impl: ({ ACTION: e, SUBRULE1: t, CONSUME: n }) => (r) => {
		let i = n(cv), a = t(Uw), o = n(lv);
		return e(() => r.astFactory.wrap(a.val, r.astFactory.sourceLocation(i, o)));
	}
}, Mw = {
	name: "quadData",
	impl: ({ ACTION: e, SUBRULE1: t, CONSUME: n }) => (r) => {
		let i = n(cv), a = e(() => r.parseMode.delete("canParseVars")), o = t(Uw);
		e(() => a && r.parseMode.add("canParseVars"));
		let s = n(lv);
		return e(() => r.astFactory.wrap(o.val, r.astFactory.sourceLocation(i, s)));
	}
};
function Nw(e, t, n, r) {
	return {
		name: e,
		impl: ({ ACTION: i, SUBRULE1: a, CONSUME: o }) => (s) => {
			let c = o(n), l = !0;
			e !== "insertData" && (l = i(() => s.parseMode.delete("canCreateBlankNodes")));
			let u = a(r);
			return e !== "insertData" && i(() => l && s.parseMode.add("canCreateBlankNodes")), i(() => s.astFactory.updateOperationInsDelDataWhere(t, u.val, s.astFactory.sourceLocation(c, u)));
		},
		gImpl: ({ SUBRULE: e, PRINT_WORD: n, PRINT_ON_EMPTY: r, PRINT_ON_OWN_LINE: i, NEW_LINE: a }) => (o, s) => {
			let { astFactory: c, indentInc: l } = s;
			c.printFilter(o, () => {
				r(t === "insertdata" ? "INSERT DATA " : t === "deletedata" ? "DELETE DATA " : "DELETE WHERE "), s[V] += l, n("{"), a();
			}), e(Uw, c.wrap(o.data, o.loc)), c.printFilter(o, () => {
				s[V] -= l, i("}");
			});
		}
	};
}
var Pw = Nw("insertData", "insertdata", Ry, Mw), Fw = Nw("deleteData", "deletedata", Fy, Mw), Iw = Nw("deleteWhere", "deletewhere", Iy, jw), Lw = {
	name: "modify",
	impl: ({ ACTION: e, CONSUME: t, SUBRULE1: n, SUBRULE2: r, OPTION1: i, OPTION2: a, OR: o }) => (s) => {
		let c = i(() => ({
			withToken: t(Py),
			graph: n(Y)
		})), { insert: l, del: u } = o([{ ALT: () => ({
			del: n(Rw),
			insert: a(() => n(zw))
		}) }, { ALT: () => ({
			insert: r(zw),
			del: void 0
		}) }]), d = n(ax);
		t(my);
		let f = n(Q);
		return e(() => s.astFactory.updateOperationModify(s.astFactory.sourceLocation(c?.withToken, u, l, f), l?.val ?? [], u?.val ?? [], f, d, c?.graph));
	},
	gImpl: ({ SUBRULE: e, PRINT_WORDS: t, PRINT_ON_EMPTY: n, NEW_LINE: r }) => (i, a) => {
		let { astFactory: o, indentInc: s } = a;
		i.graph && (o.printFilter(i, () => t("WITH")), e(Y, i.graph)), i.delete.length > 0 && (o.printFilter(i, () => {
			a[V] += s, t("DELETE", "{"), r();
		}), e(Uw, o.wrap(i.delete, i.loc)), o.printFilter(i, () => {
			a[V] -= s, n("}"), r();
		})), i.insert.length > 0 && (o.printFilter(i, () => {
			a[V] += s, t("INSERT", "{"), r();
		}), e(Uw, o.wrap(i.insert, i.loc)), o.printFilter(i, () => {
			a[V] -= s, n("} "), r();
		})), e(ax, i.from), o.printFilter(i, () => t("WHERE")), e(Q, i.where);
	}
}, Rw = {
	name: "deleteClause",
	impl: ({ ACTION: e, SUBRULE: t, CONSUME: n }) => (r) => {
		let i = n(Ly), a = e(() => r.parseMode.delete("canCreateBlankNodes")), o = t(jw);
		return e(() => a && r.parseMode.add("canCreateBlankNodes")), e(() => r.astFactory.wrap(o.val, r.astFactory.sourceLocation(i, o)));
	}
}, zw = {
	name: "insertClause",
	impl: ({ ACTION: e, SUBRULE: t, CONSUME: n }) => (r) => {
		let i = n(zy), a = t(jw);
		return e(() => r.astFactory.wrap(a.val, r.astFactory.sourceLocation(i, a)));
	}
}, Bw = {
	name: "graphOrDefault",
	impl: ({ ACTION: e, SUBRULE1: t, CONSUME: n, OPTION: r, OR: i }) => (a) => i([{ ALT: () => {
		let t = n(w_);
		return e(() => a.astFactory.graphRefDefault(a.astFactory.sourceLocation(t)));
	} }, { ALT: () => {
		let i = r(() => n(T_)), o = t(Y);
		return e(() => a.astFactory.graphRefSpecific(o, a.astFactory.sourceLocation(i, o)));
	} }])
}, Vw = {
	name: "graphRef",
	impl: ({ ACTION: e, SUBRULE: t, CONSUME: n }) => (r) => {
		let i = n(T_), a = t(Y);
		return e(() => r.astFactory.graphRefSpecific(a, r.astFactory.sourceLocation(i, a)));
	},
	gImpl: ({ SUBRULE: e, PRINT_WORD: t }) => (n, { astFactory: r }) => {
		r.printFilter(n, () => t("GRAPH")), e(Y, n.graph);
	}
}, Hw = {
	name: "graphRefAll",
	impl: ({ ACTION: e, SUBRULE: t, CONSUME: n, OR: r }) => (i) => r([
		{ ALT: () => t(Vw) },
		{ ALT: () => {
			let t = n(w_);
			return e(() => i.astFactory.graphRefDefault(i.astFactory.sourceLocation(t)));
		} },
		{ ALT: () => {
			let t = n(C_);
			return e(() => i.astFactory.graphRefNamed(i.astFactory.sourceLocation(t)));
		} },
		{ ALT: () => {
			let t = n(E_);
			return e(() => i.astFactory.graphRefAll(i.astFactory.sourceLocation(t)));
		} }
	]),
	gImpl: ({ SUBRULE: e, PRINT_WORD: t }) => (n, { astFactory: r }) => {
		r.isGraphRefSpecific(n) ? e(Vw, n) : r.isGraphRefDefault(n) ? r.printFilter(n, () => t("DEFAULT")) : r.isGraphRefNamed(n) ? r.printFilter(n, () => t("NAMED")) : r.printFilter(n, () => t("ALL"));
	}
}, Uw = {
	name: "quads",
	impl: ({ ACTION: e, SUBRULE: t, CONSUME: n, MANY: r, SUBRULE1: i, SUBRULE2: a, OPTION1: o, OPTION2: s, OPTION3: c }) => (l) => {
		let u = [], d;
		return o(() => {
			let t = i(WS);
			d = t, e(() => u.push(t));
		}), r(() => {
			let r = t(Ww);
			d = r, u.push(r), s(() => {
				let e = n(uv);
				return d = e, e;
			}), c(() => {
				let t = a(WS);
				d = t, e(() => u.push(t));
			});
		}), e(() => l.astFactory.wrap(u, l.astFactory.sourceLocation(u.at(0), d)));
	},
	gImpl: ({ SUBRULE: e }) => (t, { astFactory: n }) => {
		for (let r of t.val) n.isPattern(r) ? e(BS, r) : e(Ww, r);
	}
}, Ww = {
	name: "quadsNotTriples",
	impl: ({ ACTION: e, SUBRULE1: t, CONSUME: n, OPTION: r }) => (i) => {
		let a = n(T_), o = t(Zb);
		n(cv);
		let s = r(() => t(WS)), c = n(lv);
		return e(() => i.astFactory.graphQuads(o, s ?? i.astFactory.patternBgp([], i.astFactory.sourceLocation()), i.astFactory.sourceLocation(a, c)));
	},
	gImpl: ({ SUBRULE: e, PRINT_WORD: t, NEW_LINE: n, PRINT_ON_OWN_LINE: r }) => (i, a) => {
		let { astFactory: o, indentInc: s } = a;
		o.printFilter(i, () => t("GRAPH")), e(Xb, i.graph), o.printFilter(i, () => {
			a[V] += s, t("{"), n();
		}), e(BS, i.triples), o.printFilter(i, () => {
			a[V] -= s, r("}");
		});
	}
}, Gw = {
	name: "queryOrUpdate",
	impl: ({ ACTION: e, SUBRULE: t, OR1: n, OR2: r, MANY: i, OPTION1: a, CONSUME: o, SUBRULE2: s }) => (c) => {
		let l = t(Kb);
		return n([{ ALT: () => {
			let n = r([
				{ ALT: () => t(uw) },
				{ ALT: () => t(pw) },
				{ ALT: () => t(mw) },
				{ ALT: () => t(hw) }
			]), i = t(gw);
			return e(() => ({
				context: l,
				...n,
				type: "query",
				...i && { values: i },
				loc: c.astFactory.sourceLocation(l.at(0), n, i)
			}));
		} }, { ALT: () => {
			let n = [];
			n.push({ context: l });
			let r = !0;
			return i({
				GATE: () => r,
				DEF: () => {
					r = !1, n.at(-1).operation = t(xw), a(() => {
						o(fv), r = !0;
						let e = s(Kb);
						n.push({ context: e });
					});
				}
			}), e(() => {
				let e = {
					type: "update",
					updates: n,
					loc: c.astFactory.sourceLocation(...n.flatMap((e) => [...e.context, e.operation]))
				};
				return c.skipValidation || jb(e), e;
			});
		} }]);
	},
	gImpl: ({ SUBRULE: e }) => (t, { astFactory: n }) => {
		n.isQuery(t) ? e(lw, t) : e(bw, t);
	}
};
//#endregion
//#region ../../wt-query-graph/web/node_modules/@traqula/rules-sparql-1-1/dist/esm/lib/MinimalSparqlParser.js
function Kw(e) {
	return {
		astFactory: e.astFactory ?? new xb({ tracksSourceLocation: !1 }),
		baseIRI: e.baseIRI,
		prefixes: Object.assign(Object.create(null), e.prefixes),
		parseMode: e.parseMode ? new Set(e.parseMode) : /* @__PURE__ */ new Set(["canParseVars", "canCreateBlankNodes"]),
		skipValidation: e.skipValidation ?? !1
	};
}
function qw(e) {
	return {
		astFactory: e.astFactory ?? new xb(),
		origSource: e.origSource ?? "",
		offset: e.offset,
		[V]: e["When you use this string, you expect traqula to handle indentation after every newline"] ?? 0,
		[rg]: e[rg] ?? " ",
		indentInc: e.indentInc ?? 2
	};
}
function Jw(e) {
	return {
		...e,
		prefixes: Object.assign(Object.create(null), e.prefixes),
		parseMode: new Set(e.parseMode)
	};
}
var Yw = class {
	parser;
	defaultContext;
	coreTransformer = new fg();
	constructor(e, t = {}) {
		this.parser = e, this.defaultContext = Kw(t);
	}
	parse(e, t = {}) {
		let n = this.parser.queryOrUpdate(e, Jw({
			...this.defaultContext,
			...t
		}));
		return n.loc = this.defaultContext.astFactory.sourceLocationInlinedSource(e, n.loc, 0, 2 ** 53 - 1), n;
	}
	parsePath(e, t = {}) {
		let n = this.parser.path(e, Jw({
			...this.defaultContext,
			...t
		}));
		return n.loc = this.defaultContext.astFactory.sourceLocationInlinedSource(e, n.loc, 0, 2 ** 53 - 1), this.defaultContext.astFactory.isPathPure(n) ? {
			...n,
			prefixes: Object.create(null)
		} : n;
	}
}, Xw = [
	$,
	VC,
	HC,
	UC,
	WC,
	GC,
	IC,
	KC,
	qC,
	JC,
	YC,
	XC,
	ZC,
	Fb,
	Ib,
	Lb,
	Rb,
	zb,
	Bb,
	X,
	vx,
	yx,
	bx,
	xx,
	Sx,
	Cx,
	wx,
	Tx,
	Ex,
	Dx,
	Ox,
	kx,
	Ax,
	jx,
	Mx,
	Nx,
	Px,
	Fx
], Zw = ug.create(Xw).addMany(Ix, Lx, Rx, zx, Bx, Vx, Hx, Ux, Wx, Gx, Kx, qx, Jx, Yx, Xx, Zx, Qx, $x, eS, tS, nS, rS, iS, aS, oS, sS, cS, lS, uS, dS, fS, hS, gS, _S, bS, xS, SS, CS, wS, TS, ES, DS, Y, Hb, Ub, FC, Vb).addRule({
	name: "builtInCall",
	impl: ({ OR: e, SUBRULE: t }) => () => e(pS(t).slice(0, -2))
}), Qw = [
	eC,
	rC,
	hC,
	Xb,
	lC,
	oC,
	fC,
	YS,
	Yb,
	Gb,
	Zb,
	X,
	Y,
	Hb,
	Ub,
	Qb,
	Fb,
	Ib,
	Bb,
	Wb,
	Vb,
	Lb,
	Rb,
	zb
], $w = ug.create(Qw), eT = ug.create([
	BS,
	US,
	XS,
	uC,
	qS,
	ZS,
	QS,
	tC
]).merge($w, []).addMany(OS, NS, MS, jS, PS, IS, FS, LS, RS, iC, gC, sC, pC), tT = [
	dw,
	fw,
	_C,
	QC,
	gw
], nT = ug.create(tT).merge(Zw, []).patchRule(mS).addMany(vS, yS, Q, yC).merge(eT, []).addMany(bC, jC, xC, AC, SC, CC, MC, wC, TC, NC, PC, EC, DC, OC, kC, $C, tw, rw, aw, ew, nw, iw, ow, sw), rT = [
	WS,
	HS,
	Xb,
	YS,
	lC,
	KS,
	X,
	Qb,
	Y,
	Hb,
	Ub,
	Fb,
	Vb,
	Ib,
	Lb,
	Rb,
	zb,
	Bb,
	Wb,
	Yb,
	Gb,
	Zb,
	eC,
	rC,
	oC,
	fC,
	hC
], iT = ug.create(rT), aT = [
	cw,
	lw,
	Kb,
	uw,
	pw,
	mw,
	hw,
	gw,
	qb,
	Jb
], oT = ug.create(aT).merge(nT, []).addRule(ex).addRule(ix).addRule(tx).addRule(ox).addRule(sx).addRule(_w).merge(iT, []).addRule(vw), sT = {
	name: "update1",
	impl: ({ SUBRULE: e, OR: t }) => () => t([
		{ ALT: () => e(Sw) },
		{ ALT: () => e(ww) },
		{ ALT: () => e(Tw) },
		{ ALT: () => e(Ow) },
		{ ALT: () => e(kw) },
		{ ALT: () => e(Aw) },
		{ ALT: () => e(Ew) },
		{ ALT: () => e(Pw) },
		{ ALT: () => e(Fw) },
		{ ALT: () => e(Iw) }
	]),
	gImpl: xw.gImpl
}, cT = [
	yw,
	bw,
	Kb,
	qb,
	Jb,
	Sw,
	ww,
	Tw,
	Ow,
	kw,
	Aw,
	Ew,
	Pw,
	Fw,
	Iw,
	Y,
	Ub,
	Vw,
	Hw,
	Bw,
	Mw,
	Uw
], lT = ug.create(cT).addRule(sT).merge(iT, []).addRule(jw).addRule(Ww), uT = ug.create(lT).patchRule(xw).addMany(Lw, Rw, zw, nx, tx, ox, sx, ax, Q).merge($w, []).merge(nT, []), dT = ug.create(oT).merge(uT, []).addRule(Gw), fT = class extends Yw {
	constructor(e = {}) {
		let t = dT.build({
			...e,
			tokenVocabulary: tb.tokenVocabulary,
			queryPreProcessor: Sb
		});
		super(t, e.defaultContext);
	}
}, pT = og.create([
	Gw,
	lw,
	uw,
	pw,
	mw,
	hw,
	fw
]).addMany(bw, xw, Sw, ww, Tw, Ew, Aw, kw, Ow, Pw, Fw, Iw, Lw, Vw, Hw, Uw, Ww).addRule(DS).addMany(ix, ax).addMany(FC, $, ZC).addMany(Kb, Jb, qb, Xb, X, Qb).addMany(Fb, Y, Hb, Ub, Wb).addRule(kS).addMany(QC, $C, tw, rw, aw).addMany(BS, sC, pC, uC, gC).addMany(_C, vC, Q, bC, xC, SC, CC, wC, TC, AC, jC, MC), mT = class {
	defaultContext;
	constructor(e = {}) {
		this.defaultContext = qw(e);
	}
	generator = pT.build();
	generate(e, t = {}) {
		return this.generator.queryOrUpdate(e, {
			...this.defaultContext,
			...t
		}).trim();
	}
	generatePath(e, t = {}) {
		return this.generator.path(e, qw({
			...this.defaultContext,
			...t
		}), void 0).trim();
	}
}, hT = new fT(), gT = new mT(), _T = "https://w3id.org/rare-disease-atlas/vocab#", vT = "https://w3id.org/rare-disease-atlas/id/", yT = (e) => e.subType === "literal" ? JSON.stringify(e, (e, t) => e === "loc" ? void 0 : t) : `${e.subType}:${e.value}`, bT = (e) => `term_${encodeURIComponent(yT(e))}`, xT = (e) => e?.subType === "namedNode" && e.value.startsWith(vT) ? decodeURIComponent(e.value.slice(39)) : null, ST = (e, t) => {
	if (e && typeof e == "object") {
		t(e);
		for (let [n, r] of Object.entries(e)) n !== "loc" && (Array.isArray(r) ? r.forEach((e) => ST(e, t)) : ST(r, t));
	}
}, CT = /* @__PURE__ */ new Set([
	"studies_condition",
	"names_gene",
	"about_gene",
	"about_condition",
	"author_of",
	"principal_investigator_of",
	"sponsored_by",
	"awarded_to",
	"serves_condition",
	"serves_gene",
	"same_as",
	"held_by",
	"model_of",
	"studied_for",
	"targets",
	"resource_for",
	"funds",
	"claims_about",
	"has_phenotype",
	"orthologous_to",
	"gene_associated_with_condition",
	"candidate_same_as",
	"related_to",
	"has_associated_gene",
	"lacks_phenotype"
]), wT = (e, t) => e?.subType === "variable" && e.value === t;
function TT(e) {
	if (e?.subType !== "values" || e.variables?.length !== 1 || !wT(e.variables[0], "p") || !e.values?.length) return null;
	let t = e.values.map((e) => {
		if (Object.keys(e).length !== 1 || e.p?.subType !== "namedNode" || !e.p.value.startsWith(_T)) return null;
		let t = e.p.value.slice(42);
		return CT.has(t) ? t : null;
	});
	return t.every(Boolean) && new Set(t).size === t.length && JSON.stringify(t) === JSON.stringify([...t].sort()) ? t : null;
}
function ET(e) {
	if (e.subType !== "select" || e.distinct !== !0 || e.datasets?.clauses.length || e.variables?.length !== 3 || !e.variables.every((e, t) => wT(e, [
		"s",
		"p",
		"o"
	][t])) || Object.keys(e).some((e) => ![
		"type",
		"subType",
		"loc",
		"context",
		"distinct",
		"variables",
		"datasets",
		"where",
		"solutionModifiers"
	].includes(e))) return null;
	let t = e.solutionModifiers, n = t?.limitOffset?.limit;
	if (!Number.isInteger(n) || n < 1 || n > 160 || Object.keys(t).some((e) => e !== "limitOffset") || t.limitOffset.offset !== void 0 || Object.keys(t.limitOffset).some((e) => ![
		"type",
		"subType",
		"loc",
		"limit",
		"offset"
	].includes(e)) || e.where?.subType !== "group") return null;
	let r, i = null;
	if (e.where.patterns.length === 1 && e.where.patterns[0].subType === "union") r = e.where.patterns[0];
	else if (e.where.patterns.length === 2) {
		i = e.where.patterns[0];
		let t = e.where.patterns[1];
		r = t.subType === "union" ? t : t.subType === "group" && t.patterns.length === 1 ? t.patterns[0] : null;
	}
	if (r?.subType !== "union" || r.patterns.length < 2 || r.patterns.length > 32 || r.patterns.length % 2) return null;
	let a = i && TT(i);
	if (i && !a) return null;
	let o = i ? [i] : [], s = /* @__PURE__ */ new Map(), c = a, l = null;
	for (let [e, t] of r.patterns.entries()) {
		if (t.subType !== "group" || t.patterns.length !== (i ? 2 : 3)) return null;
		let [n, r, a] = i ? [null, ...t.patterns] : t.patterns;
		if (!i) {
			let e = TT(n);
			if (!e || c && JSON.stringify([...c].sort()) !== JSON.stringify([...e].sort())) return null;
			c = e, o.push(n);
		}
		if (r?.subType !== "bgp" || r.triples.length !== 1 || a?.subType !== "bind") return null;
		let u = r.triples[0];
		if (!wT(u.predicate, "p")) return null;
		let d, f;
		if (wT(u.object, "o") && wT(a.variable, "s")) d = u.subject, f = "outgoing";
		else if (wT(u.subject, "s") && wT(a.variable, "o")) d = u.object, f = "incoming";
		else return null;
		if (f !== (e % 2 ? "incoming" : "outgoing")) return null;
		let p;
		try {
			p = xT(d);
		} catch {
			return null;
		}
		if (!p || d.value !== `${vT}${encodeURIComponent(p).replace(/[!'()*]/g, (e) => `%${e.charCodeAt(0).toString(16).toUpperCase()}`)}` || a.expression?.subType !== "namedNode" || a.expression.value !== d.value || e % 2 && l !== p) return null;
		l = p;
		let m = s.get(p) || /* @__PURE__ */ new Set();
		if (m.has(f)) return null;
		m.add(f), s.set(p, m);
	}
	return [...s.values()].some((e) => e.size !== 2) || JSON.stringify([...s.keys()]) !== JSON.stringify([...s.keys()].sort()) ? null : {
		seed_ids: [...s.keys()],
		relations: [...c].sort(),
		directions: ["incoming", "outgoing"],
		limit: n,
		values: o
	};
}
function DT(e, t, n, r, i) {
	let a = {
		seed_ids: i.seed_ids,
		relations: i.relations,
		directions: i.directions,
		limit: i.limit
	}, o = i.seed_ids.map((e) => n.find((t) => t.id === e)?.label || e), s = o.length > 2 ? `${o.slice(0, 2).join(", ")} +${o.length - 2}` : o.join(", ");
	return {
		ast: e,
		text: t,
		graph: {
			version: 1,
			nodes: [{
				key: "neighborhood_seeds",
				ids: i.seed_ids,
				label: s,
				kind: i.seed_ids.length === 1 && n.find((e) => e.id === i.seed_ids[0])?.kind || null
			}, {
				key: "neighborhood_records",
				ids: [],
				label: "?s / ?o",
				kind: null
			}],
			edges: [{
				key: "neighborhood_outgoing",
				from: "neighborhood_seeds",
				to: "neighborhood_records",
				relation: "?p",
				step: 0
			}, {
				key: "neighborhood_incoming",
				from: "neighborhood_records",
				to: "neighborhood_seeds",
				relation: "?p",
				step: 1
			}],
			settings: {
				pattern: "neighborhood",
				limit: i.limit
			}
		},
		terms: /* @__PURE__ */ new Map(),
		editable: !1,
		inspection: r,
		neighborhood: a
	};
}
function OT(e) {
	let t = hT.parse(e);
	if (t.type !== "query" || !Array.isArray(t.context)) throw Error("unsupported_graph");
	let n = new Map(t.context.filter((e) => e.subType === "prefix").map((e) => [e.key, e.value.value]));
	if (t.context.some((e) => e.subType === "base")) throw Error("unsupported_graph");
	return ST(t, (e) => {
		if (e.subType === "namedNode" && e.prefix !== void 0) {
			if (!n.has(e.prefix)) throw Error("unsupported_graph");
			e.value = n.get(e.prefix) + e.value, delete e.prefix;
		}
	}), t;
}
function kT(e) {
	let t = [];
	function n(e, r = !1) {
		if (e.subType === "bgp") {
			t.push({
				pattern: e,
				optional: r
			});
			return;
		}
		if (["group", "optional"].includes(e.subType)) {
			e.patterns.forEach((t) => n(t, r || e.subType === "optional"));
			return;
		}
		if (![
			"filter",
			"bind",
			"values"
		].includes(e.subType)) throw Error("unsupported_graph");
	}
	return n(e.where), t;
}
function AT(e, t = []) {
	if (new TextEncoder().encode(e).length > 16384) throw Error("unsupported_graph");
	let n = OT(e);
	if (n.type !== "query" || n.subType !== "select" || n.datasets.clauses.length) throw Error("unsupported_graph");
	let r = ET(n);
	if (r) return DT(n, e, t, FT(n), r);
	let i = /* @__PURE__ */ new Map(), a = [], o = /* @__PURE__ */ new Map(), s = /* @__PURE__ */ new Map();
	ST(n.where, (e) => {
		if (e.subType === "values") for (let t of e.variables ?? []) s.set(t.value, (e.values ?? []).map((e) => xT(e[t.value])).filter(Boolean));
	});
	for (let { pattern: e, optional: r } of kT(n)) for (let n of e.triples) {
		if (n.predicate.subType !== "namedNode" || ![
			"variable",
			"namedNode",
			"literal"
		].includes(n.subject.subType) || ![
			"variable",
			"namedNode",
			"literal"
		].includes(n.object.subType)) throw Error("unsupported_graph");
		for (let e of [n.subject, n.object]) {
			let n = bT(e);
			if (o.set(n, e), !i.has(n)) {
				let r = xT(e) ? [xT(e)] : e.subType === "variable" ? s.get(e.value) ?? [] : [], a = r.length === 1 ? t.find((e) => e.id === r[0]) : void 0;
				i.set(n, {
					key: n,
					ids: r,
					label: a?.label ?? (e.subType === "variable" ? `?${e.value}` : e.value),
					kind: a?.kind ?? null
				});
			}
		}
		if (n.predicate.value === `${_T}nodeKind` && n.object.subType === "literal") {
			i.get(bT(n.subject)).kind = n.object.value;
			continue;
		}
		a.push({
			key: `edge_${a.length}`,
			from: bT(n.subject),
			to: bT(n.object),
			relation: n.predicate.value.startsWith(_T) ? n.predicate.value.slice(42) : n.predicate.value,
			step: a.length,
			optional: r
		});
	}
	let c = new Set(a.flatMap((e) => [e.from, e.to])), l = {
		version: 1,
		nodes: [...i.values()].filter((e) => c.has(e.key)),
		edges: a,
		settings: {
			pattern: "traverse",
			filters: {
				country: null,
				recruiting: null,
				kind: null
			},
			limit: n.solutionModifiers?.limitOffset?.limit ?? 100,
			reasoning: !1
		}
	};
	if (!l.nodes.length || l.nodes.length > 40 || l.edges.length > 60) throw Error("unsupported_graph");
	let u = FT(n);
	return {
		ast: n,
		graph: l,
		terms: o,
		text: e,
		inspection: u,
		editable: u.editable
	};
}
function jT(e, t, n, r = []) {
	if (e.editable === !1) throw Error("unsupported_edit");
	let i = OT(e.text), a = e.terms.get(t);
	if (!a || !["variable", "namedNode"].includes(a.subType) || !/^[a-z][a-z0-9_]*$/.test(n.relation)) throw Error("unsupported_edit");
	let o = kT(i).find(({ pattern: e }) => e.triples.some((e) => [e.subject, e.object].some((e) => yT(e) === yT(a))));
	if (!o) throw Error("unsupported_edit");
	let s = /* @__PURE__ */ new Set();
	ST(i, (e) => {
		e.subType === "variable" && s.add(e.value);
	});
	let c = 1;
	for (; s.has(`next${c}`);) c++;
	let l = n.node ? `<${vT}${encodeURIComponent(n.node.id).replace(/[!'()*]/g, (e) => `%${e.charCodeAt(0).toString(16).toUpperCase()}`)}>` : `?next${c}`, u = hT.parse(`SELECT * WHERE { ?anchor <${_T}${n.relation}> ${l} . }`).where.patterns[0].triples[0];
	if (u.subject = a, n.direction === "incoming" && ([u.subject, u.object] = [u.object, u.subject]), o.pattern.triples.push(u), !n.node) {
		if (n.target_class !== n.target_kind) throw Error("unsupported_edit");
		let e = hT.parse(`SELECT * WHERE { ?next${c} <${_T}nodeKind> ${JSON.stringify(n.target_kind)} . }`).where.patterns[0].triples[0];
		o.pattern.triples.push(e);
	}
	return AT(gT.generate(i), n.node ? [...r, n.node] : r);
}
function MT(e, t, n = !1, r = []) {
	if (e.editable === !1) throw Error("unsupported_edit");
	let i = OT(e.text), a = kT(i), o = e.graph.edges.find((e) => e.key === t), s = e.terms.get(t);
	if (n && !o || !n && !s) throw Error("unsupported_edit");
	let c = 0;
	for (let { pattern: e } of a) e.triples = e.triples.filter((e) => e.predicate.value === `${_T}nodeKind` ? n || yT(e.subject) !== yT(s) : !(n ? `edge_${c++}` === t : [e.subject, e.object].some((e) => yT(e) === yT(s))));
	let l = /* @__PURE__ */ new Set(), u = /* @__PURE__ */ new Set();
	for (let { pattern: e } of a) ST(e.triples, (e) => {
		e.subType === "variable" && l.add(e.value);
	});
	if (ST(i.where, (e) => {
		e.subType === "values" && ST(e, (e) => {
			e.subType === "variable" && l.add(e.value);
		});
	}), ST(i, (e) => {
		["filter", "bind"].includes(e.subType) && ST(e, (e) => {
			e.subType === "variable" && u.add(e.value);
		});
	}), ST(i.variables, (e) => {
		e.subType === "variable" && u.add(e.value);
	}), ST(i.solutionModifiers, (e) => {
		e.subType === "variable" && u.add(e.value);
	}), [...u].some((e) => !l.has(e))) throw Error("unsupported_edit");
	return AT(gT.generate(i), r);
}
function NT(e, t) {
	if (!Number.isInteger(t) || t < 1 || t > 160) throw Error("invalid_row_cap");
	let n = OT(e), r = hT.parse(e);
	if (r.subType !== "select") throw Error("unsupported_edit");
	return r.solutionModifiers = {
		...r.solutionModifiers,
		limitOffset: {
			...r.solutionModifiers?.limitOffset,
			limit: t
		}
	}, gT.generate(r, ET(n) ? { indentInc: 0 } : {});
}
function PT(e, t) {
	let n = OT(e), r = ET(n);
	if (!r || !Array.isArray(t) || !t.length || t.some((e) => !CT.has(e)) || new Set(t).size !== t.length) throw Error("unsupported_edit");
	let i = n.context.find((e) => e.subType === "prefix" && e.value.value === _T)?.key, a = i === void 0;
	if (i === void 0) for (i = "ra"; n.context.some((e) => e.key === i);) i += "_";
	let o = hT.parse(`PREFIX ${i}: <${_T}> SELECT ?p WHERE { VALUES ?p { ${[...t].sort().map((e) => `${i}:${e}`).join(" ")} } }`);
	a && n.context.push(o.context[0]);
	for (let e of r.values) e.values = o.where.patterns[0].values;
	return gT.generate(n, { indentInc: 0 });
}
function FT(e) {
	let t = [], n = [], r = e.subType === "select" && !e.datasets.clauses.length;
	function i(a, o = "where", s = []) {
		if (!a || typeof a != "object") return;
		let c = s;
		if (a.type === "query") {
			r = !1, c = [...s, "SUBQUERY"];
			let e = gT.generate({
				...a,
				where: {
					type: "pattern",
					subType: "group",
					loc: a.where.loc,
					patterns: []
				}
			});
			n.push({
				key: o,
				label: "SUBQUERY",
				variables: [],
				text: e
			});
		}
		if (a.type === "pattern") {
			if (a.subType === "bgp") {
				t.push({
					key: o,
					label: s.join(" / ") || "WHERE",
					triples: a.triples
				});
				let i = gT.generate({
					...e,
					context: [],
					variables: [],
					solutionModifiers: {},
					where: {
						type: "pattern",
						subType: "group",
						loc: e.where.loc,
						patterns: [a]
					}
				});
				n.push({
					key: `${o}.triples`,
					label: s.join(" / ") || "WHERE",
					variables: [],
					text: i.slice(i.indexOf("WHERE"))
				}), a.triples.some((e) => e.predicate.subType !== "namedNode") && (r = !1);
				return;
			}
			if (a.subType !== "group") {
				let t = a.subType.toUpperCase();
				c = [...s, t];
				let i = /* @__PURE__ */ new Set();
				ST(a, (e) => {
					e.subType === "variable" && i.add(`?${e.value}`);
				});
				let l = {
					...e,
					context: [],
					variables: [],
					solutionModifiers: {},
					where: {
						type: "pattern",
						subType: "group",
						loc: e.where.loc,
						patterns: [a]
					}
				}, u = gT.generate(l), d = u.indexOf("WHERE"), f = d < 0 ? u : u.slice(d);
				n.push({
					key: o,
					label: c.join(" / "),
					variables: [...i],
					text: f
				}), [
					"optional",
					"filter",
					"bind",
					"values"
				].includes(a.subType) || (r = !1);
			}
		}
		a.subType === "patternOperation" && (c = [...s, a.operator === "notexists" ? "NOT EXISTS" : "EXISTS"], r = !1);
		for (let [e, t] of Object.entries(a)) e !== "loc" && e !== "context" && (Array.isArray(t) ? t.forEach((t, n) => i(t, `${o}.${e}.${n}`, c)) : t && typeof t == "object" && i(t, `${o}.${e}`, c));
	}
	i(e.where);
	let a = gT.generate({
		...e,
		context: [],
		where: {
			type: "pattern",
			subType: "group",
			loc: e.where.loc,
			patterns: []
		}
	});
	return n.unshift({
		key: "output",
		label: e.subType.toUpperCase(),
		variables: [],
		text: a
	}), ST(e.variables, (e) => {
		e.subType === "aggregate" && (r = !1);
	}), (e.solutionModifiers?.group || e.solutionModifiers?.having) && (r = !1), {
		branches: t,
		constraints: n,
		editable: r,
		totalTriples: t.reduce((e, t) => e + t.triples.length, 0)
	};
}
function IT(e, t = [], n = 0) {
	if (new TextEncoder().encode(e).length > 262144) throw Error("unsupported_graph");
	let r = OT(e);
	if (r.type !== "query" || !["select", "ask"].includes(r.subType)) throw Error("unsupported_graph");
	let i = FT(r), a = ET(r);
	if (a) return DT(r, e, t, i, a);
	let o = i.branches[n];
	if (!o) throw Error("unsupported_graph");
	let s = /* @__PURE__ */ new Map(), c = /* @__PURE__ */ new Map(), l = [];
	for (let [e, n] of o.triples.entries()) {
		let i = [n.subject, n.object].map(bT);
		if ([n.subject, n.object].some((e) => ![
			"variable",
			"namedNode",
			"literal"
		].includes(e.subType))) continue;
		let a = new Set(i.filter((e) => !s.has(e))).size;
		if (!(s.size + a > 12 || l.length >= 16)) {
			for (let e of [n.subject, n.object]) {
				let n = bT(e), r = xT(e), i = t.find((e) => e.id === r);
				c.set(n, e);
				let a = i?.label ?? (e.subType === "variable" ? `?${e.value}` : e.value);
				s.set(n, {
					key: n,
					ids: r ? [r] : [],
					label: a.length > 120 ? `${a.slice(0, 120)}…` : a,
					kind: i?.kind ?? null
				});
			}
			l.push({
				key: `${o.key}:${e}`,
				from: i[0],
				to: i[1],
				relation: n.predicate.subType === "variable" ? `?${n.predicate.value}` : n.predicate.value ?? gT.generate({
					...r,
					context: [],
					variables: [],
					solutionModifiers: {},
					where: {
						type: "pattern",
						subType: "group",
						loc: r.where.loc,
						patterns: [{
							type: "pattern",
							subType: "bgp",
							loc: r.where.loc,
							triples: [n]
						}]
					}
				}),
				step: e
			});
		}
	}
	if (!s.size) throw Error("unsupported_graph");
	return {
		ast: r,
		terms: c,
		text: e,
		editable: !1,
		branch: n,
		inspection: i,
		graph: {
			version: 1,
			nodes: [...s.values()],
			edges: l,
			settings: {
				pattern: "traverse",
				filters: {
					country: null,
					recruiting: null,
					kind: null
				},
				limit: r.solutionModifiers?.limitOffset?.limit ?? 100,
				reasoning: !1
			}
		}
	};
}
//#endregion
//#region scripts/query-graph-view-parser.mjs
function LT(e) {
	let t = AT(e);
	return {
		...t.graph,
		nodes: t.graph.nodes.map((e) => ({
			...e,
			label: e.kind ? `${e.label} · ${e.kind}` : e.label
		})),
		edges: t.graph.edges.map((e) => ({
			...e,
			relation: `${e.optional ? "OPTIONAL · " : ""}${e.relation.replace(/^.*[#/]/, "")}`
		}))
	};
}
//#endregion
export { jT as addConnection, IT as inspectQuery, AT as parseQuery, MT as removeElement, PT as setNeighborhoodRelations, NT as setQueryLimit, LT as visualizeQuery };
