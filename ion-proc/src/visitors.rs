/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use syn::punctuated::Punctuated;
use syn::visit_mut::{VisitMut, visit_type_mut, visit_type_path_mut, visit_type_reference_mut};
use syn::{GenericArgument, PathArguments, Type, TypePath, TypeReference, parse_quote};

use crate::utils::path_ends_with;

pub(crate) struct LifetimeRemover;

impl VisitMut for LifetimeRemover {
	fn visit_type_path_mut(&mut self, ty: &mut TypePath) {
		if let Some(segment) = ty.path.segments.last_mut() {
			if let PathArguments::AngleBracketed(arguments) = &mut segment.arguments {
				let args = arguments.args.clone().into_iter().filter(|argument| match argument {
					GenericArgument::Lifetime(lt) => *lt == parse_quote!('static),
					_ => true,
				});
				arguments.args = Punctuated::from_iter(args);
			}
		}
		visit_type_path_mut(self, ty);
	}

	fn visit_type_reference_mut(&mut self, ty: &mut TypeReference) {
		if ty.lifetime != Some(parse_quote!('static)) {
			ty.lifetime = None;
		}
		visit_type_reference_mut(self, ty);
	}
}

pub(crate) struct SelfRenamer<'t> {
	pub(crate) ty: &'t Type,
}

impl VisitMut for SelfRenamer<'_> {
	fn visit_type_mut(&mut self, ty: &mut Type) {
		if let Type::Path(typ) = ty {
			if path_ends_with(&typ.path, "Self") {
				*ty = self.ty.clone();
			}
		}
		visit_type_mut(self, ty);
	}
}
