/// Compares two strings ignoring case and any non-alphanumeric characters, so
/// that `"Dynamic Stabilizer"`, `"dynamic-stabilizer"`, `"dynamic_stabilizer"`
/// and `"dynamicstabilizer"` all compare equal. Allocation-free.
///
/// `pub` only so the `str_enum!` expansion can reach it through
/// `$crate::__private`; the module it lives in is private, so this is not part
/// of the crate's public API.
pub fn normalized_eq(a: &str, b: &str) -> bool {
    fn key(s: &str) -> impl Iterator<Item = char> + '_ {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
    }

    key(a).eq(key(b))
}

/// Declares a fieldless enum together with `as_str`, `Display`, `FromStr`,
/// `TryFrom<&str>` and `TryFrom<String>` (all three parse entry points failing
/// with [`ParsingError`](crate::errors::ParsingError)), an `ALL` slice, and an
/// `EXPECTED` list built at compile time from the same literals used everywhere
/// else.
///
/// The generated enum always derives `Debug, Clone, Copy, PartialEq, Eq, Hash`,
/// so callers must not repeat those.
///
/// Extra parse-only synonyms are listed after the canonical display string, and
/// matching ignores case and separators either way:
///
/// ```
/// use generic_helpers::str_enum;
///
/// str_enum! {
///   pub enum Equipment {
///     Barbell => "Barbell" | "bb",
///     Dumbbell => "Dumbbell" | "db",
///   }
/// }
///
/// // `.ok()` because `ParsingError` is not `PartialEq`.
/// assert_eq!("bb".parse().ok(), Some(Equipment::Barbell));
/// assert_eq!(Equipment::try_from("db").ok(), Some(Equipment::Dumbbell));
/// assert_eq!(
///     Equipment::try_from(String::from("Barbell")).ok(),
///     Some(Equipment::Barbell),
/// );
/// assert_eq!(Equipment::Barbell.to_string(), "Barbell");
/// assert_eq!(Equipment::EXPECTED, "[Barbell, Dumbbell]");
/// ```
///
/// Attributes pass through on both the enum and its variants, which is how
/// callers bolt on `serde`/`ts-rs` without this crate depending on either:
///
/// ```
/// use generic_helpers::str_enum;
///
/// str_enum! {
///   /// Where an exercise sits in a movement pattern.
///   pub enum MuscleRole {
///     Primary => "Primary",
///     /// Serde would otherwise emit `"DynamicStabilizer"`, disagreeing with
///     /// `as_str`; a `#[serde(rename = "...")]` here keeps them aligned.
///     DynamicStabilizer => "Dynamic Stabilizer",
///   }
/// }
/// ```
///
/// Every path in the expansion is absolute (`$crate::…`, `::core::…`), so the
/// only thing a caller needs in scope is the macro itself.
#[macro_export]
macro_rules! str_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $(#[$head_meta:meta])*
            $head_variant:ident => $head_display:literal $(| $head_alias:literal)*,
            $(
                $(#[$variant_meta:meta])*
                $variant:ident => $display:literal $(| $alias:literal)*
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        $vis enum $name {
            $(#[$head_meta])*
            $head_variant,
            $(
                $(#[$variant_meta])*
                $variant,
            )*
        }

        impl $name {
            /// Every variant, in declaration order.
            $vis const ALL: &'static [Self] = &[Self::$head_variant, $(Self::$variant,)*];

            /// The canonical display strings, formatted for error messages.
            $vis const EXPECTED: &'static str =
                ::core::concat!("[", $head_display, $(", ", $display,)* "]");

            $vis const fn as_str(&self) -> &'static str {
                match self {
                    Self::$head_variant => $head_display,
                    $(Self::$variant => $display,)*
                }
            }

            /// Whether `s` names this variant, ignoring case and separators.
            fn accepts(&self, s: &str) -> bool {
                match self {
                    Self::$head_variant => [$head_display $(, $head_alias)*]
                        .into_iter()
                        .any(|candidate| $crate::__private::normalized_eq(candidate, s)),
                    $(Self::$variant => [$display $(, $alias)*]
                        .into_iter()
                        .any(|candidate| $crate::__private::normalized_eq(candidate, s)),)*
                }
            }
        }

        impl ::core::str::FromStr for $name {
            type Err = $crate::errors::ParsingError;

            fn from_str(s: &str) -> ::core::result::Result<Self, Self::Err> {
                Self::ALL
                    .iter()
                    .copied()
                    .find(|variant| variant.accepts(s))
                    .ok_or_else(|| $crate::errors::ParsingError::InvalidStringEnumValue {
                        value: ::std::string::String::from(s),
                        to_type: ::core::any::type_name::<Self>(),
                        expected: Self::EXPECTED,
                    })
            }
        }

        impl ::core::convert::TryFrom<&str> for $name {
            type Error = $crate::errors::ParsingError;

            /// Defers to `FromStr` so the two entry points cannot drift apart.
            fn try_from(s: &str) -> ::core::result::Result<Self, Self::Error> {
                <Self as ::core::str::FromStr>::from_str(s)
            }
        }

        impl ::core::convert::TryFrom<::std::string::String> for $name {
            type Error = $crate::errors::ParsingError;

            fn try_from(s: ::std::string::String) -> ::core::result::Result<Self, Self::Error> {
                // Repeats the lookup rather than deferring to `FromStr` so the
                // failure path can hand `s` to the error instead of copying it.
                // Binding the result first ends `find`'s borrow of `s`, which
                // `ok_or_else` then moves.
                let matched = Self::ALL.iter().copied().find(|variant| variant.accepts(&s));

                matched.ok_or_else(|| $crate::errors::ParsingError::InvalidStringEnumValue {
                    value: s,
                    to_type: ::core::any::type_name::<Self>(),
                    expected: Self::EXPECTED,
                })
            }
        }

        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}
