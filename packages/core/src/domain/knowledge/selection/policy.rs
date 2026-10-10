//! Acceptance is a measurable application policy, never proof of correctness.
use serde::{Deserialize,Serialize};
use serde_json::Value;

#[derive(Clone,Copy,Debug,Serialize,Deserialize)]
pub struct Acceptance {
    pub confidence:f64,
    pub probability:f64,
    pub margin:f64,
}
impl Default for Acceptance {
    fn default()->Self {Self{confidence:0.5,probability:0.7,margin:0.2}}
}
impl Acceptance {
    pub fn accepts(self,answer:&Value)->bool {
        let Some(choice)=answer["choice"].as_str() else{return false;};
        let Some(confidence)=answer["confidence"].as_f64().filter(|v|v.is_finite() && (0.0..=1.0).contains(v)) else{return false;};
        let Some(probabilities)=answer["probabilities"].as_object() else{return false;};
        let Some(probability)=probabilities.get(choice).and_then(Value::as_f64) else{return false;};
        let mut sum=0.0;let mut other=0.0_f64;
        for (key,value) in probabilities {
            let Some(value)=value.as_f64().filter(|v|v.is_finite() && (0.0..=1.0).contains(v)) else{return false;};
            sum+=value;if key!=choice {other=other.max(value);}
        }
        (sum-1.0).abs()<=0.02 && confidence>=self.confidence && probability>=self.probability && probability-other>=self.margin
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn invalid_distributions_and_confidence_do_not_become_accepted_choices() {
        let policy=Acceptance::default();
        assert!(policy.accepts(&json!({"choice":"a","confidence":0.8,"probabilities":{"a":0.9,"b":0.1}})));
        for answer in [json!({"choice":"a","confidence":1.4,"probabilities":{"a":1}}),
            json!({"choice":"a","confidence":0.8,"probabilities":{"a":0.9,"b":0.9}}),
            json!({"choice":"a","confidence":0.8,"probabilities":{"a":0.9,"b":-0.1}}),
            json!({"choice":"a","confidence":0.8,"probabilities":{"b":1}}),
            json!({"choice":"a","confidence":0.8,"probabilities":{"a":0.6,"b":0.4}})] {assert!(!policy.accepts(&answer));}
    }
}
